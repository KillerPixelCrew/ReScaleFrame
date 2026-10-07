// SPDX-License-Identifier: GPL-3.0-only
/**
 * @file
 * Check queued AC7 render scopes without a graphics device.
 * A synthetic native command list preserves the researched layout and defers marker
 * execution until after the CPU frame advances. Cases cover nesting, bounded tickets,
 * viewless Slate ownership and refusal to unload before commands/callbacks retire.
 */
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
// Destruction from inside a callback must fail while the current ticket is executing.
void notify(void* user, void*, const rsf_ac7_render_scope* scope, uint32_t)
{
    auto& check = *static_cast<Check*>(user);
    check.observed.push_back(scope->native_frame);
    check.callback_unload_refused &= rsf_ac7_render_scopes_destroy(check.scopes) == 0;
}
bool expect(bool ok, const char* message)
{
    if (!ok) std::fprintf(stderr, "%s\n", message);
    return ok;
}
// Save next before dispatch: executing a marker may retire its command storage.
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
    return ok ? 0 : 1;
}
