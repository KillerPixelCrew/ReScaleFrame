// SPDX-License-Identifier: MIT
#include "../runtime/orchestrator/src/fg_status_snapshot.h"
#include <cstdio>
#include <cstdlib>
#include <initializer_list>

namespace {
void check(bool ok, int line)
{
    if (ok) return;
    std::fprintf(stderr, "FG status contract failed at %d\n", line);
    std::exit(1);
}
#define CHECK(value) check((value), __LINE__)
}

int main()
{
    rsf_native_fg_status pending{};
    pending.struct_size = sizeof(pending); pending.source_frame_id = 11;
    pending.vendor.struct_size = sizeof(pending.vendor); pending.vendor.supported = 1;
    pending.vendor.max_generated_frames = 4; pending.vendor.version_id = 99;
    pending.vendor.configured_mode = RSF_FG_FIXED; pending.vendor.active = 0;
    pending.vendor.total_presented = 100; pending.vendor.valid_statistics = RSF_FG_STAT_LATENCY_AVAILABLE;
    rsf::fg_status::Completed completed;
    completed.generation = 7; completed.status = pending;
    completed.status.source_frame_id = 10; completed.status.source_qpc = 1000;
    completed.status.vendor.active = 1; completed.status.vendor.effective_mode = RSF_FG_FIXED;
    completed.status.vendor.effective_generated_frames = 1;
    completed.status.vendor.total_presented = 212; completed.status.vendor.generated_callbacks = 106;
    completed.status.vendor.valid_statistics = RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_ACTIVITY;
    auto coherent = rsf::fg_status::coherent(pending, 7, completed, 7, true, true);
    CHECK(coherent.source_frame_id == 10 && coherent.source_qpc == 1000);
    CHECK(coherent.vendor.active == 1 && coherent.vendor.total_presented == 212 && coherent.vendor.generated_callbacks == 106);
    CHECK(coherent.vendor.max_generated_frames == 4 && coherent.vendor.version_id == 99);
    CHECK(coherent.vendor.valid_statistics & RSF_FG_STAT_LATENCY_AVAILABLE);
    // A pending provider query cannot replace completed activity, but capabilities stay live.
    auto fresh = pending.vendor; fresh.max_generated_frames = 2;
    auto merged = rsf::fg_status::with_completed_activity(fresh, coherent.vendor, true, true);
    CHECK(merged.active == 1 && merged.total_presented == 212 && merged.max_generated_frames == 2);
    for (auto reason : {RSF_NATIVE_FG_REASON_RESET, RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED, RSF_NATIVE_FG_REASON_NO_MATCH}) {
        pending.reason = reason;
        coherent = rsf::fg_status::coherent(pending, 7, completed, 7, true, true);
        CHECK(!coherent.vendor.active && coherent.reason == reason && coherent.source_frame_id == 11);
    }
    pending.reason = 0; pending.last_result = RSF_BACKEND_ERROR_FEATURE_FAILED;
    CHECK(!rsf::fg_status::coherent(pending, 7, completed, 7, true, true).vendor.active);
    pending.last_result = 0;
    CHECK(!rsf::fg_status::coherent(pending, 7, completed, 7, true, false).vendor.active);
    coherent = rsf::fg_status::coherent(pending, 7, completed, 8, true, true);
    CHECK(!coherent.vendor.active && !coherent.vendor.total_presented && !coherent.source_frame_id);
    CHECK(!rsf::fg_status::coherent(pending, 7, completed, 7, false, true).vendor.active);
    auto receipt = completed;
    receipt.observe(completed.status, 7, 7, 11, true, 7);
    CHECK(!receipt.matches(7)); // Completion source differs from the prepared source.
    receipt.observe(completed.status, 7, 8, 10, true, 7);
    CHECK(!receipt.matches(7));
    receipt.observe(completed.status, 7, 7, 10, false, 7);
    CHECK(!receipt.matches(7));
    receipt.observe(completed.status, 7, 7, 10, true, 7);
    CHECK(receipt.matches(7) && receipt.status.vendor.total_presented == 212);
    completed.deactivate();
    coherent = rsf::fg_status::coherent(pending, 7, completed, 7, true, true);
    CHECK(!coherent.vendor.active && coherent.vendor.total_presented == 212); // Off/On needs a new completion.
    fresh.supported = 0;
    CHECK(!rsf::fg_status::with_completed_activity(fresh, merged, true, true).active);
    fresh.supported = 1; fresh.configured_mode = RSF_FG_OFF;
    CHECK(!rsf::fg_status::with_completed_activity(fresh, merged, true, true).active);
    std::puts("FG completed-status contract passed");
}
