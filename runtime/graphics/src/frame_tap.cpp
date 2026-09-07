// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/frame_tap.h>
#include <rescaleframe/resource_roles.h>

#include <windows.h>

#include <d3d11.h>

#include <atomic>
#include <cstdarg>
#include <cstdio>
#include <mutex>

namespace {

// ID3D11DeviceContext vtable slots, counting the three IUnknown and four ID3D11DeviceChild entries
// first. Same source as the observer's slots, tools/ghidra/build-directx-types.py.
constexpr size_t slot_ps_set_shader_resources = 8;
constexpr size_t slot_draw_indexed = 12;
constexpr size_t slot_draw = 13;
constexpr size_t slot_ps_set_constant_buffers = 16;
constexpr size_t slot_om_set_render_targets = 33;
constexpr size_t slot_om_set_render_targets_and_uavs = 34;
constexpr size_t slot_rs_set_viewports = 44;
constexpr size_t slot_rs_set_scissor_rects = 45;
constexpr size_t slot_clear_render_target_view = 50;

using ps_set_shader_resources_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                            ID3D11ShaderResourceView* const*);
using ps_set_constant_buffers_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                            ID3D11Buffer* const*);
using draw_indexed_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, INT);
using draw_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT);
using om_set_render_targets_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT,
                                                          ID3D11RenderTargetView* const*,
                                                          ID3D11DepthStencilView*);
using om_set_render_targets_and_uavs_fn = void(STDMETHODCALLTYPE*)(
    ID3D11DeviceContext*, UINT, ID3D11RenderTargetView* const*, ID3D11DepthStencilView*, UINT, UINT,
    ID3D11UnorderedAccessView* const*, const UINT*);
using rs_set_viewports_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT,
                                                     const D3D11_VIEWPORT*);
using rs_set_scissor_rects_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT,
                                                         const D3D11_RECT*);
using clear_render_target_view_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*,
                                                             ID3D11RenderTargetView*,
                                                             const FLOAT[4]);

// Every slot D3D11 allows a stage, rather than a guess at how many are used.
//
// This was 16, which was reasoning from what the set needs rather than from what the engine does.
// Unreal binds its scene textures structure, depth and the GBuffer among them, alongside the post
// process inputs, and that alone can reach past slot 16, so a window of 16 can watch a pass read
// depth and never see it. The cost of the full range is a larger shadow and a longer scan, both of
// which are cheap: only a slot that actually changed pays for a resource query, and a scan is a
// pointer test per slot.
constexpr UINT max_examined_views = D3D11_COMMONSHADER_INPUT_RESOURCE_SLOT_COUNT;

// What a watch budget holds when the caller asked for no limit. See consider_target_draw.
constexpr uint32_t unlimited_budget = 0xffffffffu;

struct Tap {
    std::mutex guard;
    bool installed = false;

    void** vtable = nullptr;
    ps_set_shader_resources_fn original_set_views = nullptr;
    ps_set_constant_buffers_fn original_set_constants = nullptr;
    draw_indexed_fn original_draw_indexed = nullptr;
    draw_fn original_draw = nullptr;
    om_set_render_targets_fn original_set_targets = nullptr;
    om_set_render_targets_and_uavs_fn original_set_targets_and_uavs = nullptr;
    rs_set_viewports_fn original_set_viewports = nullptr;
    rs_set_scissor_rects_fn original_set_scissors = nullptr;
    clear_render_target_view_fn original_clear_target = nullptr;

    rsf_frame_tap_options options{};

    // The most recent pixel stage constant buffer of the watched size. Held with a reference so it
    // survives being unbound before a callback reads it.
    ID3D11Buffer* view_constants = nullptr;

    // What the pixel stage currently has bound, as far as this module has seen it.
    //
    // The set does not arrive in one call. Unreal's D3D11 backend binds shader resources a slot at
    // a time, so every call carries one view and a rule expecting four in one call never fires. The
    // resources are still bound together at the draw, they just got there separately, so the state
    // has to be shadowed across calls and the signature looked for in the shadow.
    //
    // Nothing here is retained. A view the runtime has bound is kept alive by the runtime, and this
    // mirrors exactly what is bound, so an entry is live for as long as it is in the table. Slots
    // are cleared when unbound, which is what keeps that true.
    struct Slot {
        ID3D11ShaderResourceView* view = nullptr;
        ID3D11Texture2D* texture = nullptr;
        D3D11_TEXTURE2D_DESC description{};
        rsf_resource_role role = RSF_ROLE_UNKNOWN;
    };
    Slot slots[max_examined_views];
    // Set when a binding changed, cleared when the set has been looked at. The look happens at the
    // draw rather than at the binding, so this is what keeps a run of draws with unchanged state
    // from rescanning the slots each time.
    bool shadow_dirty = false;
    // Edge trigger on the set becoming complete.
    //
    // Keying it on the identity of the textures instead fired exactly once for a whole session: the
    // game binds the same targets every frame, so the identity never changes and the edge never
    // comes back. Completeness does come back, because the pass unbinds its inputs and the frame's
    // other passes bind their own, so the set goes incomplete between frames and this fires once
    // per frame, which is what a backend wants.
    bool signature_complete = false;
    // How many near misses have been described. Bounded so this diagnostic cannot become the
    // reason the game runs badly.
    uint32_t described = 0;

    // What the output merger has bound at render target slot 0, shadowed for the same reason the
    // shader resources are: knowing which target a draw writes needs the binding call, and asking
    // the context at the draw would put an OMGetRenderTargets and two reference counts on every
    // draw in the frame. Slot 0 only. Unreal binds several targets in the GBuffer pass and nowhere
    // in the frame's tail, which is what this is for.
    //
    // The texture is retained. Unlike a shader resource, whose view the runtime keeps alive for as
    // long as it is bound, this one is resolved once here and read at a later draw.
    ID3D11RenderTargetView* target_view = nullptr;
    ID3D11Texture2D* target_texture = nullptr;
    D3D11_TEXTURE2D_DESC target_description{};
    // Draws into the current target since it was bound. Reset by a change of binding, so it counts
    // a pass rather than a frame.
    uint32_t draws_into_target = 0;

    // Watched render targets. Compared by pointer and never dereferenced, so these are plain
    // addresses rather than references: see the note on rsf_frame_tap_watch_target.
    //
    // Atomic because they are set from whichever thread asked and read on the render thread. The
    // budget is atomic for the same reason and is only ever decremented by the render thread.
    std::atomic<void*> watch[RSF_FRAME_TAP_WATCH_SLOTS] = {};
    std::atomic<uint32_t> watch_budget[RSF_FRAME_TAP_WATCH_SLOTS] = {};
    std::atomic<uint32_t> target_draws_reported{0};

    // The substitution plan. Written under the lock by whoever sets it and read on the render
    // thread without one, which `plan_active` is what makes safe: it is only ever set to true after
    // the plan is fully written, and cleared before the plan is touched again.
    rsf_frame_tap_plan plan{};
    std::atomic<bool> plan_active{false};
    // Which of the plan's gates have opened in the current frame.
    bool gate_open[RSF_FRAME_TAP_MAX_SUBSTITUTIONS] = {};
    // Whether the render target now bound is one the plan replaced, which is what decides whether a
    // viewport is scaled.
    bool target_substituted = false;
    // The viewport and scissor rectangles the game last asked for, before any scaling. A pass that
    // draws into a substituted target and then one that does not would otherwise inherit the scaled
    // values, because the engine sets a viewport per pass and not per target.
    D3D11_VIEWPORT game_viewports[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE]{};
    UINT game_viewport_count = 0;
    D3D11_RECT game_scissors[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE]{};
    UINT game_scissor_count = 0;

    std::atomic<uint32_t> inputs_substituted{0};
    std::atomic<uint32_t> targets_redirected{0};
    std::atomic<uint32_t> gates_opened{0};

    // Counters live outside the lock. Taking a mutex on every pixel shader binding would put this
    // module on the game's hottest path for the sake of two numbers nobody reads per frame.
    //
    // `calls_seen` counts every call and `calls_inspected` only those that changed a slot. The pair
    // separates a hook that never runs from one that runs and never recognises anything, which are
    // different problems with the same symptom.
    std::atomic<uint32_t> calls_seen{0};
    std::atomic<uint32_t> calls_inspected{0};
    std::atomic<uint32_t> motion_seen{0};
    std::atomic<uint32_t> depth_seen{0};
    std::atomic<uint32_t> exposure_seen{0};
    std::atomic<uint32_t> passes{0};
    std::atomic<uint32_t> render_width{0};
    std::atomic<uint32_t> render_height{0};
};

Tap& tap()
{
    static Tap instance;
    return instance;
}

// The callback binds resources of its own, which comes straight back through these hooks. Without
// this the recognition would run on the callback's own bindings and could recurse without end.
thread_local bool inside_hook = false;

struct ReentryGuard {
    ReentryGuard() { inside_hook = true; }
    ~ReentryGuard() { inside_hook = false; }
};

// Options are written once during install and never again, so reading them without the lock is
// safe, and necessary because the interesting lines are emitted with no lock held.
void say(const Tap& self, const char* format, ...)
{
    if (!self.options.log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    self.options.log(self.options.log_user, message);
}

bool patch_slot(void** vtable, size_t index, void* replacement, void** previous)
{
    DWORD protection = 0;
    if (!VirtualProtect(&vtable[index], sizeof(void*), PAGE_EXECUTE_READWRITE, &protection)) {
        return false;
    }
    if (previous) {
        *previous = vtable[index];
    }
    vtable[index] = replacement;
    DWORD restored = 0;
    VirtualProtect(&vtable[index], sizeof(void*), protection, &restored);
    return true;
}

// The texture behind a view, with a reference the caller owns. Both GetResource and QueryInterface
// hand out references, so the intermediate one is dropped here rather than leaked per binding.
ID3D11Texture2D* texture_behind(ID3D11ShaderResourceView* view)
{
    ID3D11Resource* resource = nullptr;
    view->GetResource(&resource);
    if (!resource) {
        return nullptr;
    }
    ID3D11Texture2D* texture = nullptr;
    resource->QueryInterface(__uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&texture));
    resource->Release();
    return texture;
}

// The texture behind a render target view, with a reference the caller owns. Same shape and same
// reason as texture_behind above; the two are separate because the view types are unrelated and a
// template over them would be longer than the duplication.
ID3D11Texture2D* texture_behind_target(ID3D11RenderTargetView* view)
{
    ID3D11Resource* resource = nullptr;
    view->GetResource(&resource);
    if (!resource) {
        return nullptr;
    }
    ID3D11Texture2D* texture = nullptr;
    resource->QueryInterface(__uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&texture));
    resource->Release();
    return texture;
}

// The plan entry naming this texture, or null. `index_out` receives its position, because whether
// an entry applies yet is a property of its gate rather than of the entry.
const rsf_frame_tap_substitution* find_entry(const Tap& self, void* texture, uint32_t& index_out)
{
    if (!texture) {
        return nullptr;
    }
    for (uint32_t index = 0; index < self.plan.count; ++index) {
        if (self.plan.items[index].texture == texture) {
            index_out = index;
            return &self.plan.items[index];
        }
    }
    return nullptr;
}

bool entry_applies(const Tap& self, uint32_t index)
{
    return self.plan.items[index].after_target == nullptr || self.gate_open[index];
}

// Open any gate this render target opens, and tell the caller. Runs before the binding is
// forwarded, which is the whole point: the gate is the moment the scene is finished and nothing
// downstream has read it, and a reconstruction that ran afterwards would be a frame late.
void open_gates_for(Tap& self, ID3D11DeviceContext* context, void* texture)
{
    if (!texture) {
        return;
    }
    for (uint32_t index = 0; index < self.plan.count; ++index) {
        if (self.plan.items[index].after_target != texture || self.gate_open[index]) {
            continue;
        }
        self.gate_open[index] = true;
        self.gates_opened.fetch_add(1, std::memory_order_relaxed);
        if (self.plan.on_gate) {
            // Under the reentry guard the caller set up, so whatever this binds comes back through
            // these hooks as the tap's own work rather than as the game's.
            self.plan.on_gate(self.plan.on_gate_user, context, texture);
        }
    }
}

// Put the viewport and the scissor rectangles where the currently bound target needs them.
//
// The game asks for the resolution it believes it is drawing at. When the target under it has been
// replaced by an output resolution one, that request covers a corner of it, so it is scaled; when
// the next pass binds a target that was not replaced, the game's own numbers go back, because the
// engine sets a viewport per pass and not per target and would otherwise inherit the scaled ones.
//
// The originals are called rather than the context's own methods. Going through the context would
// come straight back into the viewport hook, which would then have to decide whether a call is the
// game's request or this putting it into effect, and that is a distinction better not to need.
void apply_viewport_policy(Tap& self, ID3D11DeviceContext* context)
{
    const float scale_x = self.target_substituted ? self.plan.viewport_scale_x : 1.0f;
    const float scale_y = self.target_substituted ? self.plan.viewport_scale_y : 1.0f;

    if (self.game_viewport_count > 0 && self.original_set_viewports) {
        D3D11_VIEWPORT scaled[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE];
        for (UINT index = 0; index < self.game_viewport_count; ++index) {
            scaled[index] = self.game_viewports[index];
            scaled[index].TopLeftX *= scale_x;
            scaled[index].TopLeftY *= scale_y;
            scaled[index].Width *= scale_x;
            scaled[index].Height *= scale_y;
        }
        self.original_set_viewports(context, self.game_viewport_count, scaled);
    }
    if (self.game_scissor_count > 0 && self.original_set_scissors) {
        D3D11_RECT scaled[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE];
        for (UINT index = 0; index < self.game_scissor_count; ++index) {
            const D3D11_RECT& source = self.game_scissors[index];
            scaled[index].left = LONG(float(source.left) * scale_x);
            scaled[index].top = LONG(float(source.top) * scale_y);
            scaled[index].right = LONG(float(source.right) * scale_x);
            scaled[index].bottom = LONG(float(source.bottom) * scale_y);
        }
        self.original_set_scissors(context, self.game_scissor_count, scaled);
    }
}

// Record what the output merger now has at render target slot 0. Called from both binding hooks,
// with the game's call already forwarded.
void shadow_render_target(Tap& self, ID3D11RenderTargetView* view)
{
    if (self.target_view == view) {
        // The same view rebound is most of what a frame does. It is not a new pass, so the draw
        // ordinal deliberately continues rather than restarting: a pass that rebinds its own
        // target between draws would otherwise report every draw as the first one.
        return;
    }
    if (self.target_texture) {
        self.target_texture->Release();
        self.target_texture = nullptr;
    }
    self.target_description = D3D11_TEXTURE2D_DESC{};
    self.target_view = view;
    self.draws_into_target = 0;
    if (!view) {
        return;
    }
    self.target_texture = texture_behind_target(view);
    if (self.target_texture) {
        self.target_texture->GetDesc(&self.target_description);
    }
}

// If this draw writes a watched target and that watch still has budget, describe it.
//
// Called from the draw hooks after the game's draw has been forwarded, so the description is of a
// draw that has already happened. Everything it reads is the shadow, which is why it costs a
// pointer compare on the draws that do not match, which is all but a handful in a frame.
void consider_target_draw(Tap& self, ID3D11DeviceContext* context, bool indexed,
                          UINT element_count)
{
    if (!self.target_texture) {
        return;
    }
    const uint32_t ordinal = self.draws_into_target++;

    uint32_t watch_index = RSF_FRAME_TAP_WATCH_SLOTS;
    for (uint32_t index = 0; index < RSF_FRAME_TAP_WATCH_SLOTS; ++index) {
        if (self.watch[index].load(std::memory_order_relaxed) == self.target_texture) {
            watch_index = index;
            break;
        }
    }
    if (watch_index == RSF_FRAME_TAP_WATCH_SLOTS) {
        return;
    }

    // The budget is spent here rather than after the callback, so a callback that takes the
    // process down cannot be reached again by the next draw.
    //
    // Zero is spent and `unlimited_budget` is the caller's "no limit", which cannot be zero for the
    // obvious reason: a limit of three counts down to zero, and a zero that also meant no limit
    // would turn every exhausted watch into an endless one on its next draw.
    uint32_t budget = self.watch_budget[watch_index].load(std::memory_order_relaxed);
    for (;;) {
        if (budget == 0) {
            return;
        }
        if (budget == unlimited_budget) {
            break;
        }
        if (self.watch_budget[watch_index].compare_exchange_weak(budget, budget - 1,
                                                                 std::memory_order_relaxed)) {
            break;
        }
    }
    if (!self.options.on_target_draw) {
        return;
    }

    rsf_frame_tap_input inputs[RSF_FRAME_TAP_MAX_INPUTS]{};
    uint32_t reported = 0;
    for (UINT index = 0; index < max_examined_views && reported < RSF_FRAME_TAP_MAX_INPUTS;
         ++index) {
        const Tap::Slot& entry = self.slots[index];
        if (!entry.view) {
            continue;
        }
        rsf_frame_tap_input& input = inputs[reported++];
        input.slot = index;
        input.texture = entry.texture;
        input.width = entry.description.Width;
        input.height = entry.description.Height;
        input.format = uint32_t(entry.description.Format);
    }

    rsf_frame_tap_target_draw report{};
    report.struct_size = sizeof(report);
    report.context = context;
    report.watch_index = watch_index;
    report.render_target = self.target_texture;
    report.target_width = self.target_description.Width;
    report.target_height = self.target_description.Height;
    report.target_format = uint32_t(self.target_description.Format);
    report.draw_index = ordinal;
    report.indexed = indexed ? 1u : 0u;
    report.element_count = element_count;
    report.input_count = reported;
    report.inputs = inputs;

    // The viewport, asked for only on a draw that is being reported. It is the one thing here that
    // the shadow cannot supply, because nothing hooks RSSetViewports, and a call per reported draw
    // is affordable where a call per draw would not be.
    D3D11_VIEWPORT viewport{};
    UINT viewport_count = 1;
    context->RSGetViewports(&viewport_count, &viewport);
    if (viewport_count >= 1) {
        report.viewport_width = uint32_t(viewport.Width);
        report.viewport_height = uint32_t(viewport.Height);
    }

    // Not a count of what the caller was told: a report the caller ignores still happened, and a
    // watch that never fires is the thing this number exists to distinguish.
    self.target_draws_reported.fetch_add(1, std::memory_order_relaxed);
    self.options.on_target_draw(self.options.on_target_draw_user, &report);
}

rsf_resource_role role_of(const D3D11_TEXTURE2D_DESC& description, const rsf_frame_shape& shape)
{
    rsf_texture_facts facts{};
    facts.struct_size = sizeof(facts);
    facts.width = description.Width;
    facts.height = description.Height;
    facts.format = static_cast<uint32_t>(description.Format);
    facts.bind_flags = description.BindFlags;
    facts.mip_levels = description.MipLevels;
    facts.array_size = description.ArraySize;
    facts.sample_count = description.SampleDesc.Count;

    rsf_role_verdict verdict{};
    verdict.struct_size = sizeof(verdict);
    if (!rsf_classify_texture(&facts, &shape, &verdict)) {
        return RSF_ROLE_UNKNOWN;
    }
    return verdict.role;
}

void STDMETHODCALLTYPE hooked_ps_set_shader_resources(ID3D11DeviceContext* context, UINT start_slot,
                                                      UINT count,
                                                      ID3D11ShaderResourceView* const* views)
{
    Tap& self = tap();
    // Forwarded first unless the plan says otherwise. The game's binding has to happen whether or
    // not this recognises it, and it has to be in place before a callback that may bind something
    // else and restore it.
    const ps_set_shader_resources_fn forward = self.original_set_views;
    if (!forward) {
        // Only reachable if a call arrived between the vtable write and the store of the original,
        // which the write order in patch_slot rules out on the architectures this ships on.
        // Dropping a binding is wrong and will show; calling through a null pointer ends the
        // process with nothing to read.
        return;
    }

    // A binding named by the plan is altered before it goes through, which is the one thing here
    // that does not merely watch. Only the views are swapped; the slots, the count and the order
    // are the game's, because a pass that reads slot 3 has a shader that says slot 3.
    ID3D11ShaderResourceView* substituted[max_examined_views];
    ID3D11ShaderResourceView* const* forwarded = views;
    if (!inside_hook && views && count > 0 && count <= max_examined_views &&
        self.plan_active.load(std::memory_order_relaxed)) {
        const ReentryGuard substitution_guard;
        bool any = false;
        for (UINT index = 0; index < count; ++index) {
            substituted[index] = views[index];
            if (!views[index]) {
                continue;
            }
            ID3D11Texture2D* texture = texture_behind(views[index]);
            if (!texture) {
                continue;
            }
            uint32_t entry_index = 0;
            const rsf_frame_tap_substitution* entry = find_entry(self, texture, entry_index);
            texture->Release();
            if (!entry || !entry->shader_view || !entry_applies(self, entry_index)) {
                continue;
            }
            substituted[index] = static_cast<ID3D11ShaderResourceView*>(entry->shader_view);
            any = true;
        }
        if (any) {
            forwarded = substituted;
            self.inputs_substituted.fetch_add(1, std::memory_order_relaxed);
        }
    }
    forward(context, start_slot, count, forwarded);

    if (inside_hook) {
        return;
    }
    const ReentryGuard guard;
    self.calls_seen.fetch_add(1, std::memory_order_relaxed);

    // Update the shadow of what is bound. Only a slot whose view actually changed costs anything:
    // a pointer compare rejects the rebinding of the same texture, which is most of what a frame
    // does, and only a genuine change pays for the resource query and the classification.
    bool changed = false;
    for (UINT index = 0; index < count; ++index) {
        const UINT slot_index = start_slot + index;
        if (slot_index >= max_examined_views) {
            break;
        }
        ID3D11ShaderResourceView* view = views ? views[index] : nullptr;
        Tap::Slot& slot = self.slots[slot_index];
        if (slot.view == view) {
            continue;
        }
        changed = true;

        if (slot.texture) {
            slot.texture->Release();
        }
        slot = Tap::Slot{};
        slot.view = view;
        if (!view) {
            continue;  // unbound, and the slot is now empty, which is what keeps the shadow honest
        }
        slot.texture = texture_behind(view);
        if (!slot.texture) {
            continue;  // a buffer or a 3D texture, neither of which is in this set
        }
        slot.texture->GetDesc(&slot.description);
    }
    if (!changed) {
        return;
    }
    self.calls_inspected.fetch_add(1, std::memory_order_relaxed);
    self.shadow_dirty = true;
}

// Decide what to bind at render target slot 0, opening any gate this binding opens.
//
// Returns the view to forward, which is the game's own unless the plan replaced it. `substituted`
// is the caller's array to build a replaced set in; only slot 0 is ever replaced, because Unreal
// binds several targets in the GBuffer pass and exactly one everywhere in the frame's tail, and a
// multiple target pass whose first target moved to another resolution is a pass that will not draw
// at all.
ID3D11RenderTargetView* const* plan_render_targets(Tap& self, ID3D11DeviceContext* context,
                                                   UINT count, ID3D11RenderTargetView* const* views,
                                                   ID3D11RenderTargetView** substituted)
{
    if (inside_hook || !views || count == 0 || !self.plan_active.load(std::memory_order_relaxed)) {
        return views;
    }
    const ReentryGuard substitution_guard;
    if (!views[0]) {
        return views;
    }
    ID3D11Texture2D* texture = texture_behind_target(views[0]);
    if (!texture) {
        return views;
    }
    // Before the substitution and before the binding is forwarded. A gate is the point in the frame
    // this binding marks, not a consequence of what gets bound there.
    open_gates_for(self, context, texture);

    uint32_t entry_index = 0;
    const rsf_frame_tap_substitution* entry = find_entry(self, texture, entry_index);
    texture->Release();
    if (!entry || !entry->render_view || !entry_applies(self, entry_index) ||
        count > D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT) {
        return views;
    }
    for (UINT index = 0; index < count; ++index) {
        substituted[index] = views[index];
    }
    substituted[0] = static_cast<ID3D11RenderTargetView*>(entry->render_view);
    self.targets_redirected.fetch_add(1, std::memory_order_relaxed);
    return substituted;
}

// Whether the target now bound was replaced, and the viewport put where that needs it. Called after
// the binding has been forwarded, so the viewport lands on the target it belongs to.
void settle_target_substitution(Tap& self, ID3D11DeviceContext* context)
{
    bool substituted = false;
    if (self.plan_active.load(std::memory_order_relaxed) && self.target_texture) {
        uint32_t entry_index = 0;
        const rsf_frame_tap_substitution* entry = find_entry(self, self.target_texture, entry_index);
        substituted = entry && entry->render_view && entry_applies(self, entry_index);
    }
    if (substituted == self.target_substituted) {
        return;
    }
    self.target_substituted = substituted;
    apply_viewport_policy(self, context);
}

void STDMETHODCALLTYPE hooked_om_set_render_targets(ID3D11DeviceContext* context, UINT count,
                                                    ID3D11RenderTargetView* const* views,
                                                    ID3D11DepthStencilView* depth)
{
    Tap& self = tap();
    const om_set_render_targets_fn forward = self.original_set_targets;
    if (!forward) {
        return;
    }
    ID3D11RenderTargetView* substituted[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT];
    forward(context, count, plan_render_targets(self, context, count, views, substituted), depth);
    if (inside_hook) {
        return;
    }
    const ReentryGuard guard;
    shadow_render_target(self, (views && count > 0) ? views[0] : nullptr);
    settle_target_substitution(self, context);
}

// The same binding by another entry point. Unreal's D3D11 backend uses it whenever a pass declares
// an unordered access view, and a shadow that only watched the first call would go stale for every
// pass that does, which in this frame includes the compute-adjacent post process work.
void STDMETHODCALLTYPE hooked_om_set_render_targets_and_uavs(
    ID3D11DeviceContext* context, UINT count, ID3D11RenderTargetView* const* views,
    ID3D11DepthStencilView* depth, UINT uav_start, UINT uav_count,
    ID3D11UnorderedAccessView* const* uavs, const UINT* initial_counts)
{
    Tap& self = tap();
    const om_set_render_targets_and_uavs_fn forward = self.original_set_targets_and_uavs;
    if (!forward) {
        return;
    }
    // D3D11_KEEP_RENDER_TARGETS_AND_DEPTH_STENCIL leaves the render targets alone, so the shadow
    // has to be left alone with them, and so does the substitution: there is nothing being bound to
    // replace. Treating it as an unbind is how a shadow starts describing a target the game is
    // still drawing into.
    const bool keeps_targets = count == D3D11_KEEP_RENDER_TARGETS_AND_DEPTH_STENCIL;
    ID3D11RenderTargetView* substituted[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT];
    ID3D11RenderTargetView* const* forwarded =
        keeps_targets ? views : plan_render_targets(self, context, count, views, substituted);
    forward(context, count, forwarded, depth, uav_start, uav_count, uavs, initial_counts);
    if (inside_hook || keeps_targets) {
        return;
    }
    const ReentryGuard guard;
    shadow_render_target(self, (views && count > 0) ? views[0] : nullptr);
    settle_target_substitution(self, context);
}

void STDMETHODCALLTYPE hooked_rs_set_viewports(ID3D11DeviceContext* context, UINT count,
                                               const D3D11_VIEWPORT* viewports)
{
    Tap& self = tap();
    const rs_set_viewports_fn forward = self.original_set_viewports;
    if (!forward) {
        return;
    }
    if (inside_hook || !self.plan_active.load(std::memory_order_relaxed)) {
        forward(context, count, viewports);
        return;
    }
    const ReentryGuard guard;
    // Remembered unscaled. This is the game's intent, and it is what has to go back when the next
    // pass binds a target that was not replaced.
    self.game_viewport_count = 0;
    if (viewports) {
        for (UINT index = 0;
             index < count && index < D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
             ++index) {
            self.game_viewports[index] = viewports[index];
            self.game_viewport_count = index + 1;
        }
    }
    if (!self.target_substituted || self.game_viewport_count == 0) {
        forward(context, count, viewports);
        return;
    }
    apply_viewport_policy(self, context);
}

void STDMETHODCALLTYPE hooked_rs_set_scissor_rects(ID3D11DeviceContext* context, UINT count,
                                                   const D3D11_RECT* rectangles)
{
    Tap& self = tap();
    const rs_set_scissor_rects_fn forward = self.original_set_scissors;
    if (!forward) {
        return;
    }
    if (inside_hook || !self.plan_active.load(std::memory_order_relaxed)) {
        forward(context, count, rectangles);
        return;
    }
    const ReentryGuard guard;
    self.game_scissor_count = 0;
    if (rectangles) {
        for (UINT index = 0;
             index < count && index < D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
             ++index) {
            self.game_scissors[index] = rectangles[index];
            self.game_scissor_count = index + 1;
        }
    }
    if (!self.target_substituted || self.game_scissor_count == 0) {
        forward(context, count, rectangles);
        return;
    }
    apply_viewport_policy(self, context);
}

// The clear has to follow the target. A replaced composite that the game never clears keeps the
// previous frame's interface, and a replaced interface target that is never cleared keeps every
// frame of it at once.
void STDMETHODCALLTYPE hooked_clear_render_target_view(ID3D11DeviceContext* context,
                                                       ID3D11RenderTargetView* view,
                                                       const FLOAT colour[4])
{
    Tap& self = tap();
    const clear_render_target_view_fn forward = self.original_clear_target;
    if (!forward) {
        return;
    }
    ID3D11RenderTargetView* target = view;
    if (!inside_hook && view && self.plan_active.load(std::memory_order_relaxed)) {
        const ReentryGuard guard;
        ID3D11Texture2D* texture = texture_behind_target(view);
        if (texture) {
            uint32_t entry_index = 0;
            const rsf_frame_tap_substitution* entry = find_entry(self, texture, entry_index);
            texture->Release();
            if (entry && entry->render_view && entry_applies(self, entry_index)) {
                target = static_cast<ID3D11RenderTargetView*>(entry->render_view);
            }
        }
    }
    forward(context, target, colour);
}

// Look at what is bound now and, if it is the set, hand it to the caller.
//
// Called from the draw hooks rather than from the binding hooks. Evaluating at the binding was the
// second thing that made this recognise nothing: Unreal binds a slot at a time, so the set is
// incomplete at every individual binding and complete only once the pass is ready to draw. Both
// halves of the earlier assumption were wrong in the same direction, that the state can be judged
// at the moment it is written rather than at the moment it is used.
void consider_bound_set(Tap& self, ID3D11DeviceContext* context)
{
    if (!self.shadow_dirty) {
        return;
    }
    self.shadow_dirty = false;

    // Judged against the presented size the caller supplied, with the render size left unknown so
    // the classifier accepts anything from half of it upwards that keeps the frame's aspect. The
    // first version took the largest bound texture as the render size, which makes it an exact
    // requirement: one full resolution texture bound alongside the half resolution scene targets
    // then rejects every one of them, and nothing ever matched.
    rsf_frame_shape shape{};
    shape.struct_size = sizeof(shape);
    shape.output_width = self.options.output_width;
    shape.output_height = self.options.output_height;

    ID3D11Texture2D* scene_color = nullptr;
    ID3D11Texture2D* history = nullptr;
    ID3D11Texture2D* motion = nullptr;
    ID3D11Texture2D* depth = nullptr;
    ID3D11Texture2D* exposure = nullptr;
    // Kept from the descriptor already read above rather than asked for again below, so the
    // qualifying path adds no D3D call of its own.
    uint32_t motion_width = 0;
    uint32_t motion_height = 0;
    uint32_t scene_color_format = 0;

    for (UINT index = 0; index < max_examined_views; ++index) {
        Tap::Slot& entry = self.slots[index];
        if (!entry.texture) {
            continue;
        }
        entry.role = role_of(entry.description, shape);
        const rsf_resource_role role = entry.role;
        if (role == RSF_ROLE_MOTION && !motion) {
            motion = entry.texture;
            motion_width = entry.description.Width;
            motion_height = entry.description.Height;
            self.motion_seen.fetch_add(1, std::memory_order_relaxed);
        } else if (role == RSF_ROLE_DEPTH && !depth) {
            depth = entry.texture;
            self.depth_seen.fetch_add(1, std::memory_order_relaxed);
        } else if (role == RSF_ROLE_EXPOSURE && !exposure) {
            exposure = entry.texture;
            self.exposure_seen.fetch_add(1, std::memory_order_relaxed);
        } else if (role == RSF_ROLE_SCENE_COLOR && !history) {
            // The second target with scene colour's shape. Which of the two holds the accumulated
            // history is not decidable from a descriptor, so this is the remaining candidate and
            // not a demonstrated history buffer.
            history = entry.texture;
        }
    }

    // Scene colour is chosen by what it is and by matching the motion target's size, in a second
    // pass because the render resolution is not known until the motion target has been found.
    //
    // It used to be taken as slot 0, on Unreal's post process input convention. The game says
    // otherwise: in the set it actually binds, slot 0 holds R10G10B10A2, which is the GBuffer's
    // normals, and the colour is a floating point target further along. Taking slot 0 would have
    // handed a backend the normal buffer, and produced an image that was wrong rather than absent.
    //
    // So: a floating point colour format, rendered into, at exactly the motion target's size.
    // R11G11B10 is what this frame uses and RGBA16F is what the full resolution captures showed,
    // so both are accepted. R10G10B10A2 deliberately is not, because that is the normals.
    if (motion) {
        for (UINT index = 0; index < max_examined_views; ++index) {
            const Tap::Slot& entry = self.slots[index];
            if (!entry.texture || entry.role != RSF_ROLE_UNKNOWN) {
                continue;
            }
            if ((entry.description.Format != DXGI_FORMAT_R11G11B10_FLOAT &&
                 entry.description.Format != DXGI_FORMAT_R16G16B16A16_FLOAT) ||
                (entry.description.BindFlags & D3D11_BIND_RENDER_TARGET) == 0 ||
                entry.description.Width != motion_width ||
                entry.description.Height != motion_height) {
                continue;
            }
            scene_color = entry.texture;
            scene_color_format = uint32_t(entry.description.Format);
            break;
        }
    }

    // Velocity, a 1x1 target and depth bound at the same time is the signature. Format rules for
    // each live in resource_roles.cpp so that this module and the classifier cannot drift apart.
    //
    // Edge triggered: the set stays bound across the draws that use it, and firing on every call
    // while it does would run a backend several times over one frame.
    // Three roles are each recognised tens of thousands of times and never together, so the
    // question is no longer whether the rules work but what the pass that uses them looks like.
    // A near miss, two of the three, is the most informative thing available: it says which
    // combinations do occur, and describing the whole bound set at that moment says what is in the
    // slots instead of the third. Bounded, because this writes a line per slot on the render
    // thread and its job is to answer one question, not to run forever.
    const uint32_t present = (motion ? 1u : 0u) + (depth ? 1u : 0u) + (scene_color ? 1u : 0u);
    if (present == 2 && self.described < 12) {
        ++self.described;
        say(self, "near miss %u: motion %s, depth %s, colour %s, exposure %s. bound set follows",
            self.described, motion ? "yes" : "no", depth ? "yes" : "no",
            scene_color ? "yes" : "no", exposure ? "yes" : "no");
        for (UINT index = 0; index < max_examined_views; ++index) {
            const Tap::Slot& entry = self.slots[index];
            if (!entry.texture) {
                continue;
            }
            say(self, "  slot %u: %ux%u format %u binds 0x%x mips %u samples %u role %u", index,
                entry.description.Width, entry.description.Height,
                unsigned(entry.description.Format), unsigned(entry.description.BindFlags),
                entry.description.MipLevels, entry.description.SampleDesc.Count,
                unsigned(entry.role));
        }
    }

    // Exposure is not part of the signature, because the game does not bind it here. The set this
    // frame actually presents is colour, depth and motion at render resolution, with the 1x1
    // exposure target bound somewhere else, presumably the tonemapper. Requiring it meant waiting
    // for a set that never arrives, and it was never a requirement in the first place: a backend
    // handed no exposure derives its own, at some cost to quality and none to running at all.
    const bool qualifies = motion && depth && scene_color;
    const bool was_complete = self.signature_complete;
    self.signature_complete = qualifies;
    if (qualifies && !was_complete) {
        const uint32_t frame_index = self.passes.fetch_add(1, std::memory_order_relaxed) + 1;

        rsf_frame_tap_pass pass{};
        pass.struct_size = sizeof(pass);
        pass.context = context;
        pass.scene_color = scene_color;
        pass.history = history;
        pass.motion = motion;
        pass.depth = depth;
        pass.exposure = exposure;
        pass.frame_index = frame_index;
        pass.scene_color_format = scene_color_format;

        pass.render_width = motion_width;
        pass.render_height = motion_height;
        const uint32_t last_width =
            self.render_width.exchange(motion_width, std::memory_order_relaxed);
        const uint32_t last_height =
            self.render_height.exchange(motion_height, std::memory_order_relaxed);

        ID3D11Buffer* constants = nullptr;
        bool live = false;
        {
            // AddRef inside the lock, because a concurrent uninstall would otherwise release the
            // last reference between the read and the retain. It is a D3D call under a lock, which
            // this project otherwise avoids, but AddRef on a buffer cannot re-enter these hooks.
            std::lock_guard<std::mutex> lock(self.guard);
            live = self.installed;
            constants = self.view_constants;
            if (constants) {
                constants->AddRef();
            }
        }
        pass.view_constants = constants;

        // Announced before the callback rather than after it, because a callback that takes the
        // process down otherwise leaves no record that the set was ever recognised. Only the first
        // pass and any change of render resolution are announced: this runs on the render thread
        // in the middle of the frame, and formatting a line per pass would cost the game more than
        // the repetition is worth.
        if (live && (frame_index == 1 || last_width != motion_width ||
                     last_height != motion_height)) {
            say(self, "reconstruction input set %u at %ux%u, %s history, %s view constants",
                frame_index, pass.render_width, pass.render_height, history ? "with" : "no",
                constants ? "with" : "no");
        }

        // No lock is held here. The callback calls back into D3D, and holding a lock across that
        // is how this project deadlocked twice.
        //
        // `live` was read under the lock and can be stale by now: uninstall can return between
        // that read and this call, and the header says an in-flight hook cannot be made to finish
        // first. This narrows the window in which a caller's callback runs after it asked to be
        // detached, it does not close it.
        if (live && self.options.on_pass) {
            self.options.on_pass(self.options.on_pass_user, &pass);
        }
        if (constants) {
            constants->Release();
        }
    }

    // Nothing is released here. The shadow owns one reference per occupied slot, taken when the
    // slot changed and dropped when it changes again or when the tap is uninstalled. Releasing per
    // call was right while the set had to arrive in one call, and would be a use after free now
    // that the entries have to survive until the slot is rebound.
}

void STDMETHODCALLTYPE hooked_draw_indexed(ID3D11DeviceContext* context, UINT index_count,
                                           UINT start_index, INT base_vertex)
{
    Tap& self = tap();
    const draw_indexed_fn forward = self.original_draw_indexed;
    if (!forward) {
        return;
    }
    forward(context, index_count, start_index, base_vertex);
    if (inside_hook) {
        return;
    }
    const ReentryGuard guard;
    consider_bound_set(self, context);
    consider_target_draw(self, context, true, index_count);
}

void STDMETHODCALLTYPE hooked_draw(ID3D11DeviceContext* context, UINT vertex_count,
                                   UINT start_vertex)
{
    Tap& self = tap();
    const draw_fn forward = self.original_draw;
    if (!forward) {
        return;
    }
    forward(context, vertex_count, start_vertex);
    if (inside_hook) {
        return;
    }
    const ReentryGuard guard;
    consider_bound_set(self, context);
    consider_target_draw(self, context, false, vertex_count);
}

void STDMETHODCALLTYPE hooked_ps_set_constant_buffers(ID3D11DeviceContext* context, UINT start_slot,
                                                      UINT count, ID3D11Buffer* const* buffers)
{
    Tap& self = tap();
    const ps_set_constant_buffers_fn forward = self.original_set_constants;
    if (!forward) {
        return;
    }
    forward(context, start_slot, count, buffers);

    if (inside_hook || !buffers || count == 0) {
        return;
    }
    const uint32_t wanted = self.options.view_constant_bytes;

    for (UINT index = 0; index < count; ++index) {
        if (!buffers[index]) {
            continue;
        }
        D3D11_BUFFER_DESC description{};
        buffers[index]->GetDesc(&description);
        if (description.ByteWidth != wanted) {
            continue;
        }

        // Keep the most recent, for the reason the observer keeps the most recent of each size:
        // the contents change every frame, so an older buffer describes a frame nobody asked
        // about. Nothing here says this is the view buffer; picking among the buffers of that size
        // is the caller's job.
        buffers[index]->AddRef();
        ID3D11Buffer* previous = nullptr;
        bool kept = false;
        {
            std::lock_guard<std::mutex> lock(self.guard);
            // A call still in flight when uninstall ran must not hand a reference back to a tap
            // that has already dropped everything: nothing would ever release it, and it would
            // outlive the module holding a game buffer alive.
            if (self.installed) {
                previous = self.view_constants;
                self.view_constants = buffers[index];
                kept = true;
            }
        }
        // Both releases sit outside the lock: a final release runs the object's destructor.
        if (!kept) {
            buffers[index]->Release();
        }
        if (previous) {
            previous->Release();
        }
        return;
    }
}

} // namespace

extern "C" rsf_frame_tap_result rsf_frame_tap_install(void* device_context,
                                                      const rsf_frame_tap_options* options)
{
    if (!device_context || !options || options->struct_size < sizeof(rsf_frame_tap_options)) {
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }
    if (options->abi_version != RSF_FRAME_TAP_ABI_VERSION) {
        return RSF_FRAME_TAP_ERROR_ABI_MISMATCH;
    }

    Tap& self = tap();
    std::lock_guard<std::mutex> lock(self.guard);
    if (self.installed) {
        return RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED;
    }

    self.options = *options;
    if (self.options.view_constant_bytes == 0) {
        self.options.view_constant_bytes = 4096;
    }

    // Only the vtable is taken. The context itself is not retained: the patch applies to every
    // context created from this runtime, so holding this one alive would extend the lifetime of a
    // game object for no benefit.
    self.vtable = *reinterpret_cast<void***>(device_context);

    // Every slot at once, undone as a unit. The bindings and the draws only mean anything together:
    // hooked bindings with an unhooked draw is a tap that runs and recognises nothing, and a
    // hooked draw with an unhooked output merger reports draws into a target it cannot name. So a
    // partial patch is not a degraded tap, it is a confusing one, and it is rolled back.
    const struct {
        size_t index;
        void* replacement;
        void** original;
    } patches[] = {
        {slot_ps_set_shader_resources, reinterpret_cast<void*>(&hooked_ps_set_shader_resources),
         reinterpret_cast<void**>(&self.original_set_views)},
        {slot_ps_set_constant_buffers, reinterpret_cast<void*>(&hooked_ps_set_constant_buffers),
         reinterpret_cast<void**>(&self.original_set_constants)},
        {slot_draw_indexed, reinterpret_cast<void*>(&hooked_draw_indexed),
         reinterpret_cast<void**>(&self.original_draw_indexed)},
        {slot_draw, reinterpret_cast<void*>(&hooked_draw),
         reinterpret_cast<void**>(&self.original_draw)},
        {slot_om_set_render_targets, reinterpret_cast<void*>(&hooked_om_set_render_targets),
         reinterpret_cast<void**>(&self.original_set_targets)},
        {slot_om_set_render_targets_and_uavs,
         reinterpret_cast<void*>(&hooked_om_set_render_targets_and_uavs),
         reinterpret_cast<void**>(&self.original_set_targets_and_uavs)},
        {slot_rs_set_viewports, reinterpret_cast<void*>(&hooked_rs_set_viewports),
         reinterpret_cast<void**>(&self.original_set_viewports)},
        {slot_rs_set_scissor_rects, reinterpret_cast<void*>(&hooked_rs_set_scissor_rects),
         reinterpret_cast<void**>(&self.original_set_scissors)},
        {slot_clear_render_target_view,
         reinterpret_cast<void*>(&hooked_clear_render_target_view),
         reinterpret_cast<void**>(&self.original_clear_target)},
    };
    constexpr size_t patch_count = sizeof(patches) / sizeof(patches[0]);

    for (size_t applied = 0; applied < patch_count; ++applied) {
        if (patch_slot(self.vtable, patches[applied].index, patches[applied].replacement,
                       patches[applied].original)) {
            continue;
        }
        while (applied-- > 0) {
            patch_slot(self.vtable, patches[applied].index, *patches[applied].original, nullptr);
            *patches[applied].original = nullptr;
        }
        return RSF_FRAME_TAP_ERROR_PATCH_FAILED;
    }

    self.installed = true;
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_uninstall(void)
{
    Tap& self = tap();
    ID3D11Buffer* constants = nullptr;
    {
        std::lock_guard<std::mutex> lock(self.guard);
        if (!self.installed) {
            return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
        }
        // Cleared before the vtable goes back, so a call already inside a hook stops substituting
        // rather than reaching for views the caller is about to release.
        self.plan_active.store(false, std::memory_order_relaxed);
        patch_slot(self.vtable, slot_ps_set_shader_resources,
                   reinterpret_cast<void*>(self.original_set_views), nullptr);
        patch_slot(self.vtable, slot_ps_set_constant_buffers,
                   reinterpret_cast<void*>(self.original_set_constants), nullptr);
        patch_slot(self.vtable, slot_draw_indexed,
                   reinterpret_cast<void*>(self.original_draw_indexed), nullptr);
        patch_slot(self.vtable, slot_draw, reinterpret_cast<void*>(self.original_draw), nullptr);
        patch_slot(self.vtable, slot_om_set_render_targets,
                   reinterpret_cast<void*>(self.original_set_targets), nullptr);
        patch_slot(self.vtable, slot_om_set_render_targets_and_uavs,
                   reinterpret_cast<void*>(self.original_set_targets_and_uavs), nullptr);
        patch_slot(self.vtable, slot_rs_set_viewports,
                   reinterpret_cast<void*>(self.original_set_viewports), nullptr);
        patch_slot(self.vtable, slot_rs_set_scissor_rects,
                   reinterpret_cast<void*>(self.original_set_scissors), nullptr);
        patch_slot(self.vtable, slot_clear_render_target_view,
                   reinterpret_cast<void*>(self.original_clear_target), nullptr);
        // The originals are deliberately kept. A call that entered a hook before the vtable was
        // restored still has to forward, and clearing them turns that race from a stale hook into a
        // null call. They stay valid for as long as the runtime is loaded, and a later install
        // reads them from the vtable again.
        constants = self.view_constants;
        self.view_constants = nullptr;
        self.installed = false;
    }
    if (constants) {
        constants->Release();
    }

    // The shadow holds one reference per occupied slot. Released after the vtable is restored, so a
    // hook still in flight cannot find a slot emptied underneath it, which narrows the same window
    // uninstall already has rather than opening a new one.
    for (Tap::Slot& slot : self.slots) {
        if (slot.texture) {
            slot.texture->Release();
        }
        slot = Tap::Slot{};
    }
    self.shadow_dirty = false;
    self.signature_complete = false;

    // Same order and the same reason for the render target shadow, which holds the one other
    // reference this module takes. The watches are cleared with it: they name textures the caller
    // owns, and leaving them set across an uninstall would have a later install start matching
    // addresses from a session that has ended.
    if (self.target_texture) {
        self.target_texture->Release();
        self.target_texture = nullptr;
    }
    self.target_view = nullptr;
    self.target_description = D3D11_TEXTURE2D_DESC{};
    self.draws_into_target = 0;
    for (uint32_t index = 0; index < RSF_FRAME_TAP_WATCH_SLOTS; ++index) {
        self.watch[index].store(nullptr, std::memory_order_relaxed);
        self.watch_budget[index].store(0, std::memory_order_relaxed);
    }
    self.plan = rsf_frame_tap_plan{};
    for (bool& gate : self.gate_open) {
        gate = false;
    }
    self.target_substituted = false;
    self.game_viewport_count = 0;
    self.game_scissor_count = 0;
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_plan(const rsf_frame_tap_plan* plan)
{
    Tap& self = tap();
    std::lock_guard<std::mutex> lock(self.guard);
    if (!self.installed) {
        return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
    }
    if (!plan) {
        // Cleared first, so the render thread stops reading the plan before it is overwritten.
        self.plan_active.store(false, std::memory_order_relaxed);
        self.plan = rsf_frame_tap_plan{};
        for (bool& gate : self.gate_open) {
            gate = false;
        }
        self.target_substituted = false;
        return RSF_FRAME_TAP_OK;
    }
    if (plan->struct_size < sizeof(rsf_frame_tap_plan) || plan->count == 0 ||
        plan->count > RSF_FRAME_TAP_MAX_SUBSTITUTIONS || plan->viewport_scale_x <= 0.0f ||
        plan->viewport_scale_y <= 0.0f) {
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }
    for (uint32_t index = 0; index < plan->count; ++index) {
        const rsf_frame_tap_substitution& item = plan->items[index];
        // A named texture with nothing to put in its place is the half working case this refuses:
        // it reads like an intervention and does nothing, which is worse than saying no.
        if (!item.texture || (!item.shader_view && !item.render_view)) {
            return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
        }
    }

    self.plan_active.store(false, std::memory_order_relaxed);
    self.plan = *plan;
    for (bool& gate : self.gate_open) {
        gate = false;
    }
    self.target_substituted = false;
    self.plan_active.store(true, std::memory_order_relaxed);
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_end_frame(void)
{
    Tap& self = tap();
    if (!self.plan_active.load(std::memory_order_relaxed)) {
        return RSF_FRAME_TAP_OK;
    }
    // No lock. This is called from the present hook on the render thread, which is the same thread
    // the gates are opened on, and taking the tap's lock on a per frame path would put the render
    // thread behind whichever worker happens to be reading status.
    for (bool& gate : self.gate_open) {
        gate = false;
    }
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_watch_target(uint32_t index, void* texture,
                                                           uint32_t limit)
{
    if (index >= RSF_FRAME_TAP_WATCH_SLOTS) {
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }
    Tap& self = tap();
    // The lock guards `installed`, and taking it here cannot deadlock against the callback this
    // may be called from: the draw path holds no lock while it calls out.
    std::lock_guard<std::mutex> lock(self.guard);
    if (!self.installed) {
        return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
    }
    // The budget first. Setting the texture first would let the render thread spend a budget that
    // belongs to the previous watch on the first draw after the store.
    self.watch_budget[index].store(
        texture ? (limit == 0 ? unlimited_budget : limit) : 0u, std::memory_order_relaxed);
    self.watch[index].store(texture, std::memory_order_relaxed);
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_get_status(rsf_frame_tap_status* status)
{
    if (!status || status->struct_size < sizeof(rsf_frame_tap_status)) {
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }
    Tap& self = tap();
    std::lock_guard<std::mutex> lock(self.guard);
    status->installed = self.installed ? 1u : 0u;
    status->calls_seen = self.calls_seen.load(std::memory_order_relaxed);
    status->calls_inspected = self.calls_inspected.load(std::memory_order_relaxed);
    status->motion_seen = self.motion_seen.load(std::memory_order_relaxed);
    status->depth_seen = self.depth_seen.load(std::memory_order_relaxed);
    status->exposure_seen = self.exposure_seen.load(std::memory_order_relaxed);
    status->passes_seen = self.passes.load(std::memory_order_relaxed);
    status->render_width = self.render_width.load(std::memory_order_relaxed);
    status->render_height = self.render_height.load(std::memory_order_relaxed);
    status->target_draws_reported = self.target_draws_reported.load(std::memory_order_relaxed);
    status->plan_set = self.plan_active.load(std::memory_order_relaxed) ? 1u : 0u;
    status->inputs_substituted = self.inputs_substituted.load(std::memory_order_relaxed);
    status->targets_redirected = self.targets_redirected.load(std::memory_order_relaxed);
    status->gates_opened = self.gates_opened.load(std::memory_order_relaxed);
    return RSF_FRAME_TAP_OK;
}
