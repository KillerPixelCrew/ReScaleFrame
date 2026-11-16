// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/backends/common/generation_state.h"
#include <atomic>
#include <barrier>
#include <cstdio>
#include <latch>
#include <thread>

namespace {
int failures{};
void check(bool ok,const char* text) { if (!ok) { ++failures; std::printf("FAIL: %s\n",text); } }
void exercise(bool fsr) {
    rsf::GenerationState state;
    rsf_fg_status initial{}; initial.struct_size=sizeof(initial); initial.supported=1;
    initial.total_presented=100; initial.latency_paced_frames=7;
    state.initialize(initial);
    rsf_fg_options options{}; options.struct_size=sizeof(options); options.abi_version=RSF_FG_ABI_VERSION;
    options.mode=RSF_FG_FIXED; options.generated_frames=3;
    const uint32_t generated=fsr ? 1u : 2u;
    const uint32_t reflex=fsr ? RSF_REFLEX_OFF : RSF_REFLEX_ON;
    state.configured(options,fsr ? RSF_FG_OFF : RSF_FG_FIXED,fsr ? 0u : generated,reflex,true);
    auto source=state.read();
    check(source.options.generated_frames==3 && source.status.effective_generated_frames==(fsr ? 0u : 2u),
        "requested count stays distinct from provider-configured effective count");
    check(state.stage_prepare(source.revision,40,RSF_FG_FIXED,generated) &&
        state.complete_prepare(source.revision,40,true,RSF_FG_FIXED,generated),"actual prepare completion establishes history");
    rsf::GenerationState::Observation observation{};
    observation.active=true; observation.total_presented_delta=5;
    observation.valid_statistics=RSF_FG_STAT_ACTIVITY|RSF_FG_STAT_TOTAL_PRESENTED;
    if (fsr) {
        observation.replace_statistics=observation.replace_callbacks=true;
        observation.generated_callbacks=9; observation.present_callbacks=20;
        observation.valid_statistics|=RSF_FG_STAT_PRESENT_CALLBACKS;
    }
    auto published=state.observed(source.revision,observation);
    check(published.active && published.total_presented==105 && published.generated_presented==0,
        "observed presented delta is separate from unconfirmed generated display count");
    if (fsr) {
        observation.generated_callbacks=12; observation.present_callbacks=24;
        observation.total_presented_delta=0; observation.valid_statistics=RSF_FG_STAT_ACTIVITY;
        published=state.observed(source.revision,observation);
        check(published.generated_callbacks==12 && published.present_callbacks==24 && published.total_presented==105 &&
            published.valid_statistics==RSF_FG_STAT_ACTIVITY,"FSR callback observation never substitutes for missing DXGI count");
    } else {
        auto warning=rsf::GenerationState::Observation{}; warning.vendor_status=7;
        published=state.observed(source.revision,warning);
        check(!published.active && published.vendor_status==7 && published.total_presented==105 &&
            published.valid_statistics==observation.valid_statistics,"XeSS warning observation retains totals/statistic validity without false activity");
        warning.vendor_status=-3; published=state.observed(source.revision,warning);
        check(!published.active && published.vendor_status==-3 && published.total_presented==105,
            "XeSS failed query observation does not add a successful present count");
        warning.vendor_status=-7; warning.total_presented_delta=3;
        warning.valid_statistics=RSF_FG_STAT_ACTIVITY|RSF_FG_STAT_TOTAL_PRESENTED;
        published=state.observed(source.revision,warning);
        check(published.vendor_status==-7 && !published.active && published.total_presented==108,
            "successful query with interpolation failure still carries its real presented count");
    }
    std::latch entered(1), finish(1); bool staged{}, accepted{};
    std::thread preparing([&] {
        const auto before=state.read();
        staged=state.stage_prepare(before.revision,41,RSF_FG_FIXED,generated);
        entered.count_down(); finish.wait();
        accepted=state.complete_prepare(before.revision,41,true,RSF_FG_FIXED,generated);
    });
    entered.wait(); const auto old_poll=state.read(); state.abort(); finish.count_down(); preparing.join();
    const auto aborted=state.read();
    check(staged && !accepted && !aborted.history_valid && !aborted.prepared_enabled && !aborted.status.active &&
        aborted.last_prepare==41,"abort during SDK gap wins against late success and consumes source ordering");
    observation.active=true; observation.total_presented_delta=1; observation.valid_statistics|=RSF_FG_STAT_TOTAL_PRESENTED;
    published=state.observed(old_poll.revision,observation);
    check(!published.active && published.total_presented==(fsr ? 106u : 109u),"stale active observation retains real counts but cannot undo abort");
    check(!state.reset_history(old_poll.revision),"resize/history reset cannot absorb an intervening abort revision");
    source=state.read();
    check(state.reset_history(source.revision) && !state.read().history_valid,"same-owner resize invalidates history without changing revision");
    check(!state.stage_prepare(old_poll.revision,42,RSF_FG_FIXED,generated),"stale source staging refuses");
    check(state.stage_prepare(source.revision,42,RSF_FG_FIXED,generated) &&
        state.complete_prepare(source.revision,42,true,RSF_FG_FIXED,generated),"fresh source can rebuild history with reset");
    if (!fsr) {
        const auto before=state.read(); options.frame_limit_us=1000;
        state.configured(options,RSF_FG_FIXED,generated,RSF_REFLEX_ON,false);
        check(state.read().history_valid && state.read().revision==before.revision,"XeLL-only configure preserves valid generation history");
        state.abort(); state.configured(options,RSF_FG_FIXED,generated,RSF_REFLEX_ON,false);
        check(!state.read().history_valid && !state.read().prepared_enabled,"XeLL-only publication cannot resurrect an abort");
    }
    source=state.read();
    check(state.stage_prepare(source.revision,43,RSF_FG_FIXED,generated),"prepare starts before mode change");
    options.mode=RSF_FG_OFF; state.configured(options,RSF_FG_OFF,0,reflex,true);
    check(!state.complete_prepare(source.revision,43,true,RSF_FG_FIXED,generated) &&
        state.read().status.effective_mode==RSF_FG_OFF,"generation change rejects late effective mode/history publication");
    const auto disabled=state.read();
    check(state.stage_prepare(disabled.revision,44,RSF_FG_OFF,0) &&
        state.complete_prepare(disabled.revision,44,false,RSF_FG_OFF,0) && !state.read().history_valid,
        "disabled source consumes ordering without creating prepared history");

    const auto disabled_before_abort=state.read(); state.abort();
    check(!state.stage_prepare(disabled_before_abort.revision,45,RSF_FG_OFF,0) &&
        !state.complete_prepare(disabled_before_abort.revision,45,false,RSF_FG_OFF,0) && state.read().last_prepare==45 &&
        !state.read().history_valid,"late disabled-source completion preserves consumed ordering without undoing abort");

    rsf::GenerationState concurrent; concurrent.initialize(initial);
    options.mode=RSF_FG_FIXED; concurrent.configured(options,RSF_FG_FIXED,generated,reflex,true);
    constexpr uint32_t iterations=4096;
    std::barrier<> start(3); std::atomic<bool> done{}, coherent{true};
    std::thread publisher([&] {
        start.arrive_and_wait();
        for (uint32_t i=1;i<=iterations;++i) {
            const auto before=concurrent.read(); rsf::GenerationState::Observation sample{};
            sample.vendor_status=int32_t(i); sample.active=true; sample.total_presented_delta=1;
            sample.valid_statistics=RSF_FG_STAT_ACTIVITY|RSF_FG_STAT_TOTAL_PRESENTED;
            sample.replace_statistics=sample.replace_callbacks=fsr;
            sample.generated_callbacks=i; sample.present_callbacks=i*2;
            (void)concurrent.observed(before.revision,sample);
        }
        done.store(true,std::memory_order_release);
    });
    std::thread aborter([&] {
        start.arrive_and_wait();
        for (uint32_t i=0;i<iterations;++i) { concurrent.abort(); if (!fsr) concurrent.paced(); }
    });
    std::thread reader([&] {
        start.arrive_and_wait();
        do {
            const auto copy=concurrent.read(); const auto sequence=uint64_t(copy.status.vendor_status);
            if (copy.status.active || copy.status.total_presented!=100+sequence || copy.status.generated_presented ||
                copy.options.mode!=copy.status.configured_mode ||
                (fsr ? (copy.status.generated_callbacks!=sequence || copy.status.present_callbacks!=sequence*2) :
                    (copy.status.generated_callbacks || copy.status.present_callbacks))) coherent.store(false);
        } while (!done.load(std::memory_order_acquire));
    });
    publisher.join(); aborter.join(); reader.join();
    const auto final=concurrent.read();
    check(coherent.load() && final.status.total_presented==100+iterations,"concurrent provider status and absolute/delta accounting copies remain coherent");
    check(final.status.latency_paced_frames==7+(fsr ? 0u : iterations),"XeLL accounting merges with observations while FSR adds no fake pace count");
    std::printf("Completed %s CPU state cases; no SDK/device calls executed\n",fsr ? "FSR" : "XeSS");
}
}
int main() {
    exercise(true); exercise(false);
    std::puts(failures ? "Generation state concurrency FAILED" : "PASS: FSR/XeSS copied-state invalidation and distinct observation semantics");
    return failures ? 1 : 0;
}
