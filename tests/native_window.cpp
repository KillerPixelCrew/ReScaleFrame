// SPDX-License-Identifier: MIT
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include <dxgi.h>
#include <cstdio>
#include <thread>

namespace {
int failures = 0;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", message);
        ++failures;
    }
}

// A borrowed COM interface can have a different address from its canonical IUnknown.
struct FakeUnknown final : IUnknown {
    FakeUnknown* identity = this;
    ULONG references = 1;
    unsigned queries = 0;
    bool available = true;

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** out) override
    {
        ++queries;
        *out = nullptr;
        if (iid != __uuidof(IUnknown) || !available) return E_NOINTERFACE;
        *out = static_cast<IUnknown*>(identity);
        identity->AddRef();
        return S_OK;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return ++references; }
    ULONG STDMETHODCALLTYPE Release() override { return --references; }
};

rsf_game_render_pass window_pass(IUnknown* swapchain)
{
    rsf_game_render_pass pass{};
    pass.struct_size = sizeof(pass);
    pass.role = RSF_GAME_RENDER_WINDOW;
    pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
    pass.session_id = 10;
    pass.source_frame_id = 20;
    pass.scope_id = 30;
    pass.viewport_key = 40;
    pass.window_key = 50;
    pass.rhi_viewport_key = 60;
    pass.swapchain = swapchain;
    return pass;
}

void scope_contract()
{
    FakeUnknown chain, alias, other;
    alias.identity = &chain;
    auto outer = window_pass(&chain);
    auto inner = window_pass(&other);
    ++inner.scope_id;
    rsf_game_render_pass match{};
    match.struct_size = sizeof(match);

    check(!rsf_native_window_match(&chain, &match), "no scope before begin");
    auto short_pass = outer;
    --short_pass.struct_size;
    check(!rsf_native_window_scope(&short_pass, 1), "short scope descriptor is refused");
    check(rsf_native_window_scope(&outer, 1) == 1, "enter outer scope");
    check(rsf_native_window_match(&alias, &match) == 1 && match.swapchain == &chain &&
        match.scope_id == outer.scope_id, "match an aliased IUnknown and return the leased pass");
    check(chain.references == 1 && alias.references == 1, "matching does not retain COM references");

    chain.available = false;
    check(!rsf_native_window_match(&alias, &match) && chain.references == 1,
        "owned QueryInterface failure releases the observed identity");
    chain.available = true;
    check(rsf_native_window_scope(&inner, 1) == 1 && !rsf_native_window_match(&chain, &match) &&
        rsf_native_window_match(&other, &match) == 1 && match.scope_id == inner.scope_id,
        "only the top nested scope matches");
    check(rsf_native_window_scope(&inner, 0) == 1 && rsf_native_window_match(&chain, &match) == 1,
        "ending the inner scope restores the outer lease");

    check(rsf_native_window_scope(&inner, 1) == 1, "enter a second inner scope");
    check(!rsf_native_window_scope(&outer, 0) && !rsf_native_window_match(&other, &match) &&
        !rsf_native_window_match(&chain, &match) && !rsf_native_window_scope(&inner, 0),
        "an out-of-order end discards every borrowed scope");
    check(rsf_native_window_scope(&outer, 1) == 1, "fresh begin after mismatched end");
    auto aliased_end = outer;
    aliased_end.swapchain = &alias;
    check(!rsf_native_window_scope(&aliased_end, 0) && !rsf_native_window_match(&chain, &match),
        "a scope end must carry the original leased pointer");

    for (uint64_t i = 1; i <= 32; ++i) {
        outer.scope_id = i;
        check(rsf_native_window_scope(&outer, 1) == 1, "enter a tracked scope");
    }
    check(!rsf_native_window_scope(&inner, 1) && !rsf_native_window_scope(&inner, 1),
        "count each untracked overflow scope");
    check(!rsf_native_window_match(&chain, &match) && !rsf_native_window_scope(&inner, 0) &&
        !rsf_native_window_match(&chain, &match), "partial overflow unwind still hides tracked scopes");
    check(!rsf_native_window_scope(&inner, 0) && rsf_native_window_match(&chain, &match) == 1 &&
        match.scope_id == 32, "balanced overflow unwind restores the tracked top");
    for (uint64_t i = 32; i != 0; --i) {
        outer.scope_id = i;
        check(rsf_native_window_scope(&outer, 0) == 1, "end a tracked scope");
    }
    check(!rsf_native_window_match(&chain, &match), "no lease remains after all scopes complete");
}

void present_contract()
{
    FakeUnknown chain, alias, surface;
    alias.identity = &chain;
    const auto window = window_pass(&chain);
    auto scene = window;
    scene.role = RSF_GAME_RENDER_FINAL_SCENE;
    scene.submission_id = 70;
    scene.view_key = 80;
    scene.native_frame = 90;
    scene.scene_surface = &surface;
    auto binding = window;
    binding.role = RSF_GAME_RENDER_TEXTURE_BINDING;
    binding.sampled_texture = &surface;
    check(rsf_native_scene_pass(&scene, 1) == 1 && rsf_native_scene_pass(&scene, 0) == 1 &&
        rsf_native_scene_texture_binding(&binding) == 1, "production scene owner records sampled composition");
    check(rsf_native_window_scope(&window, 1) == 1, "enter Present owner scope");

    rsf_native_present_record record{};
    record.struct_size = sizeof(record);
    const rsf_observer_present_event begin{sizeof(begin), 0, &alias, 1, 0, 0, 1};
    auto end = begin;
    end.completed = 1;
    end.swapchain = &chain;
    end.result = S_OK;
    LARGE_INTEGER before_begin{}, before_end{}, after_end{}, frequency{};
    const auto query_count = chain.queries + alias.queries;
    check(!rsf_native_window_present(nullptr), "null event is refused");
    auto invalid = begin;
    --invalid.struct_size;
    check(!rsf_native_window_present(&invalid), "truncated event is refused");
    invalid = begin;
    ++invalid.method;
    check(!rsf_native_window_present(&invalid) && chain.queries + alias.queries == query_count,
        "unknown Present method is refused before COM");
    check(QueryPerformanceCounter(&before_begin) != 0 && QueryPerformanceFrequency(&frequency) != 0,
        "sample native QPC before the callback pair");
    check(rsf_native_window_present(&begin) == 1 && !rsf_native_window_last_present(&record),
        "Present1 pre reports pending ownership without a completed result");

    bool isolated = false;
    std::thread other_thread([&] {
        rsf_game_render_pass match{};
        match.struct_size = sizeof(match);
        rsf_native_present_record other_record{};
        other_record.struct_size = sizeof(other_record);
        isolated = !rsf_native_window_match(&chain, &match) && !rsf_native_window_present(&end) &&
            !rsf_native_window_last_present(&other_record);
        rsf_native_window_reset();
    });
    other_thread.join();
    check(isolated, "scope and pending Present are local to the graphics thread");

    auto test = begin;
    test.flags = DXGI_PRESENT_TEST;
    check(!rsf_native_window_present(&test), "DXGI test pre is ignored");
    test.completed = 1;
    check(!rsf_native_window_present(&test), "DXGI test post is ignored");
    check(QueryPerformanceCounter(&before_end) != 0, "sample native QPC before post");
    check(rsf_native_window_present(&end) == 1 && rsf_native_window_last_present(&record) == 1,
        "aliased pre and post complete the same scope");
    check(QueryPerformanceCounter(&after_end) != 0, "sample native QPC after post");
    check(record.matched == 1 && record.completed == 1 && record.accepted == 1 &&
        record.result == S_OK && record.method == 1 && record.sync_interval == 1 &&
        record.session_id == window.session_id && record.source_frame_id == window.source_frame_id &&
        record.scope_id == window.scope_id && record.window_key == window.window_key &&
        record.viewport_key == window.viewport_key, "completed record keeps source and window identity");
    check(record.begin_qpc >= uint64_t(before_begin.QuadPart) &&
        record.begin_qpc <= uint64_t(before_end.QuadPart) &&
        record.end_qpc >= uint64_t(before_end.QuadPart) && record.end_qpc <= uint64_t(after_end.QuadPart) &&
        record.qpc_frequency == uint64_t(frequency.QuadPart), "record uses actual pre/post QPC and frequency");
    check(record.scene_matched == 1 && record.scene_texture_bound == 1 && record.scene_direct_present == 0 &&
        record.scene_submission_id == scene.submission_id && record.scene_view_key == scene.view_key &&
        record.scene_native_frame == scene.native_frame, "record keeps the exact sampled scene dispatch");

    check(rsf_native_window_present(&begin) == 1 && !rsf_native_window_last_present(&record),
        "new pre replaces the old completed result");
    check(!rsf_native_scene_pass(&scene, 1), "duplicate primary scene is ambiguous");
    end.result = S_FALSE;
    check(rsf_native_window_present(&end) == 1 && rsf_native_window_last_present(&record) == 1 &&
        record.matched == 1 && record.result == S_FALSE && !record.accepted &&
        !record.scene_matched && !record.scene_texture_bound && !record.scene_direct_present,
        "post revalidates scene identity and only S_OK counts as accepted");

    auto changed = end;
    changed.method = 0;
    check(rsf_native_window_present(&begin) == 1 && !rsf_native_window_present(&changed) &&
        !rsf_native_window_last_present(&record) && !rsf_native_window_present(&end),
        "mismatched post clears the pending pair");
    check(rsf_native_window_present(&begin) == 1 && rsf_native_window_scope(&window, 0) == 1 &&
        !rsf_native_window_present(&end), "post rejects a retired window lease");
    check(rsf_native_window_present(&begin) == 1 && rsf_native_window_present(&end) == 1 &&
        rsf_native_window_last_present(&record) == 1 && !record.matched && !record.session_id,
        "a genuine unmatched pair keeps its result without a source identity");
    --record.struct_size;
    check(!rsf_native_window_last_present(&record), "record output capacity is checked");
    ++record.struct_size;
    rsf_native_window_reset();
    check(!rsf_native_window_last_present(&record), "reset clears the completed record");
    rsf_native_scene_reset();
}
}

int main()
{
    scope_contract();
    present_contract();
    if (failures) return 1;
    std::puts("PASS: native window leases, COM identity, Present QPC, and scene dispatch");
    return 0;
}
