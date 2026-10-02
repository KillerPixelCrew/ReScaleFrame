// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/render_links.h>
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <thread>

namespace {
void require(bool value, int line)
{
    if (!value) { std::fprintf(stderr, "render link check failed at %d\n", line); std::exit(1); }
}
#define CHECK(value) require((value), __LINE__)
rsf_render_link output()
{
    return {sizeof(rsf_render_link), RSF_RENDER_LINK_ABI_VERSION, 0, 0, 0, 0, 0};
}
}

int main()
{
    rsf_render_links* links = nullptr;
    CHECK(rsf_render_links_create(0, 2, &links) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !links);
    CHECK(rsf_render_links_create(42, 65, &links) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !links);
    CHECK(rsf_render_links_create(42, 2, &links) == RSF_BACKEND_OK && links);
    auto old_frame = output();
    auto next_frame = output();
    constexpr uint64_t source = UINT64_C(4294967297);
    CHECK(rsf_render_links_bind(links, 100, source, 7, &old_frame) == RSF_BACKEND_OK);
    CHECK(rsf_render_links_bind(links, 200, source + 1, 7, &next_frame) == RSF_BACKEND_OK);

    // The CPU has advanced before the queued render consumer runs. Lookup must carry
    // the earlier identity, without truncating to an engine's 32-bit frame counter.
    auto consumed = output();
    std::thread renderer([&] {
        CHECK(rsf_render_links_lookup(links, 100, 7, &consumed) == RSF_BACKEND_OK);
    });
    renderer.join();
    CHECK(consumed.source_frame_id == source && consumed.session_id == 42);
    CHECK(consumed.submission_id == old_frame.submission_id);
    CHECK(next_frame.submission_id > consumed.submission_id);
    auto refused = output();
    CHECK(rsf_render_links_bind(links, 100, source + 2, 7, &refused) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(!refused.producer_key && !refused.source_frame_id);
    CHECK(rsf_render_links_bind(links, 300, source + 2, 7, &refused) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_render_links_destroy(links) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_render_links_lookup(links, 100, 8, &consumed) == RSF_BACKEND_ERROR_STALE_RESOURCES);
    CHECK(!consumed.source_frame_id);

    auto wrong = old_frame;
    wrong.session_id = 43;
    CHECK(rsf_render_links_release(links, &wrong) == RSF_BACKEND_ERROR_STALE_RESOURCES);
    CHECK(rsf_render_links_release(links, &old_frame) == RSF_BACKEND_OK);
    auto reused = output();
    CHECK(rsf_render_links_bind(links, 100, source + 3, 8, &reused) == RSF_BACKEND_OK);
    CHECK(reused.submission_id > next_frame.submission_id);
    CHECK(rsf_render_links_release(links, &old_frame) == RSF_BACKEND_ERROR_STALE_RESOURCES);
    CHECK(rsf_render_links_lookup(links, 100, 8, &consumed) == RSF_BACKEND_OK);
    CHECK(consumed.source_frame_id == source + 3);
    CHECK(rsf_render_links_release(links, &reused) == RSF_BACKEND_OK);
    CHECK(rsf_render_links_release(links, &next_frame) == RSF_BACKEND_OK);

    // Multiple views/submissions in one app frame keep separate lifetime tickets.
    auto first_view = output();
    auto second_view = output();
    CHECK(rsf_render_links_bind(links, 100, source + 4, 8, &first_view) == RSF_BACKEND_OK);
    CHECK(rsf_render_links_bind(links, 200, source + 4, 8, &second_view) == RSF_BACKEND_OK);
    CHECK(first_view.source_frame_id == second_view.source_frame_id);
    CHECK(first_view.submission_id != second_view.submission_id);
    CHECK(rsf_render_links_release(links, &second_view) == RSF_BACKEND_OK);
    CHECK(rsf_render_links_release(links, &first_view) == RSF_BACKEND_OK);

    auto bad = output();
    bad.struct_size = sizeof(bad) - 1;
    CHECK(rsf_render_links_lookup(links, 100, 8, &bad) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    bad = output(); bad.abi_version = 0;
    CHECK(rsf_render_links_bind(links, 100, source, 8, &bad) == RSF_BACKEND_ERROR_ABI_MISMATCH);
    CHECK(rsf_render_links_release(links, &bad) == RSF_BACKEND_ERROR_ABI_MISMATCH);

    // Two producers racing on one live engine key must not replace each other's frame.
    std::atomic<uint32_t> successes{0};
    auto a = output(); auto b = output();
    std::thread one([&] { if (rsf_render_links_bind(links, 500, source + 5, 8, &a) == RSF_BACKEND_OK) ++successes; });
    std::thread two([&] { if (rsf_render_links_bind(links, 500, source + 6, 8, &b) == RSF_BACKEND_OK) ++successes; });
    one.join(); two.join();
    CHECK(successes == 1);
    CHECK(rsf_render_links_release(links, a.producer_key ? &a : &b) == RSF_BACKEND_OK);
    CHECK(rsf_render_links_destroy(links) == RSF_BACKEND_OK);
    std::puts("Delayed render identity, bounded lifetime and stale-ticket checks passed.");
    return 0;
}
