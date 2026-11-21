// SPDX-License-Identifier: MIT
#include <rescaleframe/ac7_render_scope.h>
#include <cstddef>
#include <cstdio>
#include <vector>

struct Command {
    Command* next = nullptr;
    void (*execute)(void*, Command*) = nullptr;
};
struct NativeList {
    Command* root = nullptr;
    Command** tail = &root;
    bool executing = false;
    unsigned char padding[3]{};
    uint32_t count = 0, uid = 1;
};
static_assert(offsetof(NativeList, count) == 0x14 && offsetof(NativeList, uid) == 0x18);
struct Check {
    rsf_ac7_render_scopes* scopes = nullptr;
    std::vector<uint64_t> observed;
    bool callback_unload_refused = true;
};
void notify(void* user, void*, const rsf_ac7_render_scope* scope, uint32_t)
{
    auto& check = *static_cast<Check*>(user);
    check.observed.push_back(scope->native_frame);
    check.callback_unload_refused &= rsf_ac7_render_scopes_destroy(check.scopes) == 0;
}
void throwing_pass(void*, void*, const rsf_ac7_render_scope*, uint32_t) { throw 1; }
struct Lease {
    int resolves = 0, releases = 0;
    bool throw_resolve = false;
};
void lease_resolve(void* object, rsf_ac7_render_scope* scope)
{
    auto& lease = *static_cast<Lease*>(object);
    ++lease.resolves;
    if (lease.throw_resolve) throw 2;
    scope->native_frame += 100;
}
void lease_release(void* object)
{
    ++static_cast<Lease*>(object)->releases;
    throw 3;
}
bool expect(bool ok, const char* message)
{
    if (!ok) std::fprintf(stderr, "%s\n", message);
    return ok;
}
void run(NativeList& list)
{
    list.executing = true;
    for (auto* command = list.root; command;) {
        auto* next = command->next;
        command->execute(&list, command);
        command = next;
    }
    list.executing = false; list.root = nullptr; list.tail = &list.root; list.count = 0; ++list.uid;
}
int main()
{
    Check check; NativeList list;
    if (!expect(rsf_ac7_render_scopes_create(7, 2, notify, &check, &check.scopes) != 0, "create")) return 1;
    rsf_ac7_render_scope frame{}; frame.struct_size = sizeof(frame); frame.session_id = 7;
    frame.family_key = 0x100; frame.view_key = 0x200; frame.native_frame = 41;
    rsf_ac7_render_ticket *outer = nullptr, *inner = nullptr;
    bool ok = expect(rsf_ac7_render_scope_open(check.scopes, &list, &frame, &outer) != 0, "enqueue outer");
    frame.native_frame = 42; frame.view_key = 0x300;
    ok &= expect(rsf_ac7_render_scope_open(check.scopes, &list, &frame, &inner) != 0, "enqueue inner");
    rsf_ac7_render_ticket* refused = nullptr;
    ok &= expect(!rsf_ac7_render_scope_open(check.scopes, &list, &frame, &refused), "bounded pending work");
    ok &= expect(rsf_ac7_render_scope_close(inner, &list) != 0 && rsf_ac7_render_scope_close(outer, &list) != 0, "close reserved markers");
    frame.native_frame = 900; // CPU has advanced; queued metadata must stay 41/42.
    rsf_ac7_render_scopes_quiesce(check.scopes);
    ok &= expect(!rsf_ac7_render_scopes_destroy(check.scopes), "unload must wait for engine execution");
    ok &= expect(!rsf_ac7_render_scope_open(check.scopes, &list, &frame, &refused), "quiesce stops producers");
    run(list);
    ok &= expect(check.observed == std::vector<uint64_t>({41, 42, 41, 0}), "queued identity and enclosing-scope restoration");
    ok &= expect(check.callback_unload_refused, "callbacks must finish before unload");
    rsf_ac7_render_scope current{}; current.struct_size = sizeof(current);
    ok &= expect(!rsf_ac7_render_scope_current(check.scopes, &current), "closed stream has no stale view");
    ok &= expect(rsf_ac7_render_scopes_destroy(check.scopes) != 0, "quiescent destruction");

    // Use the shapes emitted by the native Slate hooks. Neither operation owns a scene view.
    check = {};
    ok &= expect(rsf_ac7_render_scopes_create(7, 2, notify, &check, &check.scopes) != 0, "create Slate scopes");
    rsf_ac7_render_scope window{}; window.struct_size = sizeof(window); window.session_id = 7;
    window.role = RSF_GAME_RENDER_WINDOW; window.source_frame_id = 8;
    window.viewport_key = 0x100; window.window_key = 0x200; window.rhi_viewport_key = 0x300; window.pass_key = 0x400;
    auto invalid = window; invalid.window_key = 0;
    ok &= expect(!rsf_ac7_render_scope_open(check.scopes, &list, &invalid, &refused), "window needs window owner");
    invalid = window; invalid.rhi_viewport_key = 0;
    ok &= expect(!rsf_ac7_render_scope_open(check.scopes, &list, &invalid, &refused), "window needs native viewport");
    invalid = window; invalid.source_frame_id = 0;
    ok &= expect(!rsf_ac7_render_scope_open(check.scopes, &list, &invalid, &refused), "window needs source frame");
    ok &= expect(rsf_ac7_render_scope_open(check.scopes, &list, &window, &outer) != 0, "queue viewless window");
    auto binding = window; binding.role = RSF_GAME_RENDER_TEXTURE_BINDING; binding.rhi_viewport_key = 0;
    invalid = binding; invalid.viewport_key = 0;
    ok &= expect(!rsf_ac7_render_scope_open(check.scopes, &list, &invalid, &refused), "binding needs source viewport");
    ok &= expect(rsf_ac7_render_scope_open(check.scopes, &list, &binding, &inner) != 0, "queue viewless texture binding");
    ok &= expect(rsf_ac7_render_scope_close(inner, &list) && rsf_ac7_render_scope_close(outer, &list), "close Slate scopes");
    run(list);
    ok &= expect(check.observed.size() == 4, "Slate callbacks executed with no scene view");
    invalid = window; invalid.role = RSF_GAME_RENDER_SUBMISSION;
    ok &= expect(!rsf_ac7_render_scope_open(check.scopes, &list, &invalid, &refused), "scene submission still needs its view");
    rsf_ac7_render_scopes_quiesce(check.scopes);
    ok &= expect(rsf_ac7_render_scopes_destroy(check.scopes) != 0, "Slate quiescent destruction");

    // Consumer exceptions stay in render_scope.cpp's leaves (the owner is Rust): a throwing pass
    // callback keeps the state callback, a throwing resolve skips that marker's stack change and
    // callbacks, and a throwing release still retires the ticket.
    check = {};
    ok &= expect(rsf_ac7_render_scopes_create_passes(7, 2, notify, throwing_pass, &check, &check.scopes) != 0,
        "create pass scopes");
    Lease lease;
    const rsf_ac7_scope_lease owned{&lease, lease_resolve, lease_release};
    frame.native_frame = 5;
    rsf_ac7_render_ticket* leased = nullptr;
    ok &= expect(rsf_ac7_render_scope_open_leased(check.scopes, &list, &frame, &owned, &leased) != 0 &&
        rsf_ac7_render_scope_close(leased, &list) != 0, "queue leased scope");
    run(list);
    ok &= expect(check.observed == std::vector<uint64_t>({105, 0}), "pass exception keeps state callbacks");
    ok &= expect(lease.resolves == 2 && lease.releases == 1, "lease resolved per marker, released once");
    lease.throw_resolve = true;
    ok &= expect(rsf_ac7_render_scope_open_leased(check.scopes, &list, &frame, &owned, &leased) != 0 &&
        rsf_ac7_render_scope_close(leased, &list) != 0, "queue throwing lease");
    run(list);
    ok &= expect(check.observed.size() == 2, "throwing resolve skips both markers");
    ok &= expect(lease.resolves == 4 && lease.releases == 2, "throwing release still retires");
    current.struct_size = sizeof(current);
    ok &= expect(!rsf_ac7_render_scope_current(check.scopes, &current), "no scope left open");
    rsf_ac7_render_scopes_quiesce(check.scopes);
    ok &= expect(rsf_ac7_render_scopes_destroy(check.scopes) != 0, "exception paths retired every ticket");
    return ok ? 0 : 1;
}
