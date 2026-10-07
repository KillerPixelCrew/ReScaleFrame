// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/ac7_render_scope.h>
#include <atomic>
#include <cstddef>
#include <cstring>
#include <limits>
#include <mutex>
#include <new>
#include <vector>

// Private commands match the researched UE4.18 linked-list ABI. Tickets own both markers on the
// heap; the final marker releases them after execution, independently of the engine memory stack.
struct NativeCommand {
    NativeCommand* next = nullptr;
    void (*execute)(void*, NativeCommand*) = nullptr;
};
static_assert(offsetof(NativeCommand, next) == 0 && offsetof(NativeCommand, execute) == 8);

// Producer admission and current execution stack share a guard. Capacity bounds outstanding
// tickets and pre-reserves stack storage before markers can execute on the RHI stream.
struct rsf_ac7_render_scopes {
    std::mutex guard;
    uint64_t session = 0, serial = 0;
    uint32_t capacity = 0, outstanding = 0;
    bool accepting = true;
    rsf_ac7_render_scope_fn callback = nullptr;
    rsf_ac7_render_scope_fn pass_callback = nullptr;
    void* user = nullptr;
    std::vector<rsf_ac7_render_scope> stack;
};
struct ScopeCommand : NativeCommand {
    rsf_ac7_render_ticket* ticket = nullptr;
    bool begin = false;
};
// Copied pass metadata plus optional resources survives CPU recording until the matching end.
struct rsf_ac7_render_ticket {
    rsf_ac7_render_scopes* owner = nullptr;
    rsf_ac7_render_scope scope{};
    rsf_ac7_scope_lease lease{};
    ScopeCommand begin, end;
    bool closed = false;
};

namespace {
// Frame, window and view-backed passes have different identity requirements; no raw engine view
// pointer is required for a window or full-frame marker.
bool valid_identity(const rsf_ac7_render_scope& scope)
{
    if (scope.role == RSF_GAME_RENDER_FRAME) return scope.source_frame_id != 0;
    // Slate window and texture-binding work has no scene view. Its queued ownership is
    // established by the source, game viewport and window instead. Resources resolve from
    // the lease at execution time, so they need not be available while recording.
    if (scope.role == RSF_GAME_RENDER_WINDOW || scope.role == RSF_GAME_RENDER_TEXTURE_BINDING)
        return scope.source_frame_id && scope.viewport_key && scope.window_key && scope.pass_key &&
            (scope.role != RSF_GAME_RENDER_WINDOW || scope.rhi_viewport_key);
    return scope.family_key && scope.view_key;
}
// Root +0, tail link +8, executing +0x10, NumCommands +0x14, UID +0x18.
// The native iterator saves Next before invoking execute, allowing the final marker to retire
// its own heap allocation. Engine reset owns only its memory stack, not these private records.
bool append(void* native, NativeCommand* command)
{
    if (!native || !command) return false;
    auto* bytes = static_cast<unsigned char*>(native);
    NativeCommand** tail = nullptr;
    uint32_t count = 0;
    std::memcpy(&tail, bytes + 8, sizeof(tail));
    std::memcpy(&count, bytes + 0x14, sizeof(count));
    if (bytes[0x10] || !tail || *tail || count == std::numeric_limits<uint32_t>::max()) return false;
    *tail = command;
    tail = &command->next;
    std::memcpy(bytes + 8, &tail, sizeof(tail));
    ++count;
    std::memcpy(bytes + 0x14, &count, sizeof(count));
    return true;
}
// Resolve leased resources at both edges. Pass callbacks receive the original ticket identity;
// state callbacks receive the restored enclosing scope when an end marker pops the stack.
void execute_scope(void* command_list, NativeCommand* base) noexcept
{
    auto* command = static_cast<ScopeCommand*>(base);
    auto* ticket = command->ticket;
    auto& owner = *ticket->owner;
    rsf_ac7_render_scope selected{};
    bool notify = false;
    try {
        // Output allocation may be published after begin was queued. End is appended only after
        // publication, so resolve it again before notifying the matching pass consumer.
        if (ticket->lease.resolve) ticket->lease.resolve(ticket->lease.object, &ticket->scope);
        {
            std::lock_guard<std::mutex> lock(owner.guard);
            if (command->begin) {
                owner.stack.push_back(ticket->scope); // Capacity was reserved at creation.
                selected = ticket->scope; notify = true;
            } else {
                if (!owner.stack.empty() && owner.stack.back().scope_id == ticket->scope.scope_id) {
                    owner.stack.pop_back();
                    if (!owner.stack.empty()) selected = owner.stack.back();
                    notify = true;
                }
            }
        }
        if (notify && owner.pass_callback) {
            try { owner.pass_callback(owner.user, command_list, &ticket->scope, command->begin ? 1u : 0u); }
            catch (...) { /* A consumer refusal must not suppress the scope-state restoration. */ }
        }
        if (notify && owner.callback) {
            try { owner.callback(owner.user, command_list, &selected, command->begin ? 1u : 0u); }
            catch (...) { /* Observers cannot interrupt native work or ticket retirement. */ }
        }
    } catch (...) {
        // No exception crosses the engine's native command ABI. A rejected callback cannot own
        // the engine list or prevent native work from executing.
    }
    if (!command->begin) {
        if (ticket->lease.release) ticket->lease.release(ticket->lease.object);
        { std::lock_guard<std::mutex> lock(owner.guard); --owner.outstanding; }
        delete ticket;
    }
}
}

extern "C" int rsf_ac7_render_scopes_create(uint64_t session, uint32_t capacity,
    rsf_ac7_render_scope_fn callback, void* user, rsf_ac7_render_scopes** out) try
{
    if (!out || !session || !capacity || capacity > 4096) return 0;
    *out = nullptr;
    auto* s = new rsf_ac7_render_scopes;
    try { s->stack.reserve(capacity); } catch (...) { delete s; return 0; }
    s->session = session; s->capacity = capacity; s->callback = callback; s->user = user;
    *out = s; return 1;
}
catch (...) { return 0; }

extern "C" int rsf_ac7_render_scope_open(rsf_ac7_render_scopes* s, void* list,
    const rsf_ac7_render_scope* scope, rsf_ac7_render_ticket** out)
{ return rsf_ac7_render_scope_open_leased(s, list, scope, nullptr, out); }
extern "C" int rsf_ac7_render_scopes_create_passes(uint64_t session, uint32_t capacity,
    rsf_ac7_render_scope_fn state_callback, rsf_ac7_render_scope_fn pass_callback,
    void* user, rsf_ac7_render_scopes** out)
{
    if (!rsf_ac7_render_scopes_create(session, capacity, state_callback, user, out)) return 0;
    (*out)->pass_callback = pass_callback;
    return 1;
}

extern "C" int rsf_ac7_render_scope_open_leased(rsf_ac7_render_scopes* s, void* list,
    const rsf_ac7_render_scope* scope, const rsf_ac7_scope_lease* lease, rsf_ac7_render_ticket** out) try
{
    if (!s || !list || !scope || !out || scope->struct_size < sizeof(*scope) ||
        scope->session_id != s->session || !valid_identity(*scope)) return 0;
    *out = nullptr;
    auto* ticket = new(std::nothrow) rsf_ac7_render_ticket;
    if (!ticket) return 0;
    {
        std::lock_guard<std::mutex> lock(s->guard);
        if (!s->accepting || s->outstanding == s->capacity) { delete ticket; return 0; }
        ticket->owner = s; ticket->scope = *scope; ticket->scope.scope_id = ++s->serial;
        if (lease) ticket->lease = *lease;
        ticket->begin.ticket = ticket->end.ticket = ticket;
        ticket->begin.begin = true;
        ticket->begin.execute = ticket->end.execute = execute_scope;
        if (!append(list, &ticket->begin)) { delete ticket; return 0; }
        ++s->outstanding;
    }
    *out = ticket; return 1;
}
catch (...) { return 0; }

extern "C" int rsf_ac7_render_scope_close(rsf_ac7_render_ticket* ticket, void* list)
{
    if (!ticket || !list || ticket->closed) return 0;
    ticket->closed = true;
    if (!append(list, &ticket->end)) { ticket->closed = false; return 0; }
    return 1;
}
extern "C" int rsf_ac7_render_scope_current(rsf_ac7_render_scopes* s, rsf_ac7_render_scope* out) try
{
    if (!s || !out || out->struct_size < sizeof(*out)) return 0;
    std::lock_guard<std::mutex> lock(s->guard);
    if (s->stack.empty()) return 0;
    *out = s->stack.back(); return 1;
}
catch (...) { return 0; }
extern "C" void rsf_ac7_render_scopes_quiesce(rsf_ac7_render_scopes* s) try
{
    if (!s) return;
    std::lock_guard<std::mutex> lock(s->guard); s->accepting = false;
}
catch (...) {}
extern "C" int rsf_ac7_render_scopes_idle(rsf_ac7_render_scopes* s) try
{
    if (!s) return 1;
    std::lock_guard<std::mutex> lock(s->guard);
    return !s->accepting && !s->outstanding && s->stack.empty();
}
catch (...) { return 0; }
extern "C" int rsf_ac7_render_scopes_destroy(rsf_ac7_render_scopes* s) try
{
    if (!s) return 1;
    {
        std::lock_guard<std::mutex> lock(s->guard);
        if (s->accepting || s->outstanding || !s->stack.empty()) return 0;
    }
    delete s; return 1;
}
catch (...) { return 0; }
