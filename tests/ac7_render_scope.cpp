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
    // Rust fixtures cover list membership, nesting, admission and retirement. This fixture
    // drives real C++ exceptions through the leased callbacks on the native iterator ABI.
    Check check;
    NativeList list;
    if (!expect(rsf_ac7_render_scopes_create_passes(7, 2, notify, throwing_pass, &check,
        &check.scopes) != 0, "create pass scopes")) return 1;
    rsf_ac7_render_scope frame{};
    frame.struct_size = sizeof(frame); frame.session_id = 7;
    frame.family_key = 0x100; frame.view_key = 0x200;
    bool ok = true;
    Lease lease;
    const rsf_ac7_scope_lease owned{&lease, lease_resolve, lease_release};
    frame.native_frame = 5;
    ok &= expect(rsf_ac7_render_scope_enqueue_leased(check.scopes, &list, &frame, &owned) != 0, "queue leased scope");
    run(list);
    ok &= expect(check.observed == std::vector<uint64_t>({105, 0}), "pass exception keeps state callbacks");
    ok &= expect(lease.resolves == 2 && lease.releases == 1, "lease resolved per marker, released once");
    lease.throw_resolve = true;
    ok &= expect(rsf_ac7_render_scope_enqueue_leased(check.scopes, &list, &frame, &owned) != 0, "queue throwing lease");
    run(list);
    ok &= expect(check.observed.size() == 2, "throwing resolve skips both markers");
    ok &= expect(lease.resolves == 4 && lease.releases == 2, "throwing release still retires");
    rsf_ac7_render_scope current{}; current.struct_size = sizeof(current);
    ok &= expect(check.callback_unload_refused, "callbacks must finish before unload");
    ok &= expect(!rsf_ac7_render_scope_current(check.scopes, &current), "no scope left open");
    rsf_ac7_render_scopes_quiesce(check.scopes);
    ok &= expect(rsf_ac7_render_scopes_destroy(check.scopes) != 0, "exception paths retired every ticket");
    return ok ? 0 : 1;
}
