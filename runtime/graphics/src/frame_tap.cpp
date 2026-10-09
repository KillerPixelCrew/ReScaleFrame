// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/frame_tap.h>
#include <rescaleframe/log.h>
#include <rescaleframe/resource_roles.h>

#include "vtable_patch.h"

#include <windows.h>

#include <d3d11.h>
#include <d3d11_1.h>

#include <algorithm>
#include <atomic>
#include <bit>
#include <cmath>
#include <cstring>
#include <iterator>
#include <mutex>
#include <unordered_map>

namespace {

// ID3D11DeviceContext vtable slots, counting the three IUnknown and four ID3D11DeviceChild entries
// first. Same source as the observer's slots, tools/ghidra/build-directx-types.py.
constexpr size_t slot_ps_set_shader_resources = 8;
constexpr size_t slot_ps_set_samplers = 10;
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
using ps_set_samplers_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                    ID3D11SamplerState* const*);
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
    ps_set_samplers_fn original_set_samplers = nullptr;

    // Texture LOD bias for material sampling, see rsf_frame_tap_set_sampler_bias. While a bias is
    // set, the sampler hook swaps the game's samplers for biased clones. The bound samplers are
    // asked of the context when the bias starts or ends rather than shadowed, so with no bias the
    // hook is a plain forward. Clones are kept across scopes, keyed by the game's sampler, which is
    // pinned so its address cannot be reused while it keys an entry.
    struct BiasedSampler {
        ID3D11SamplerState* clone = nullptr;
        float bias = 0;
    };
    std::mutex sampler_guard;
    float sampler_bias = 0;
    std::atomic<bool> sampler_bias_active{false};
    std::unordered_map<ID3D11SamplerState*, BiasedSampler> biased_samplers;
    std::unordered_map<ID3D11SamplerState*, ID3D11SamplerState*> clone_originals;

    // Borrowed shadow of live IA/VS bindings, never retained as a deferred draw record.
    rsf_frame_tap_geometry geometry{};
    ID3D11Buffer* pixel_constants[14]{};
    bool geometry_valid = false;
    ID3D11DepthStencilView* geometry_depth = nullptr;
    void* extra_originals[16]{};
    // Originals of the pass-through hooks: the flush-class calls and the members of the
    // work-submission family nothing else needed. Indexed as `pass_hooks` lists them.
    void* pass_originals[16]{};
    // Every slot patched, with where its original lives, so the whole table can be re-applied when
    // the runtime rewrites it and restored as a unit on uninstall. See `refresh_hooks`.
    struct Patch {
        size_t slot = 0;
        void* replacement = nullptr;
        void** original = nullptr;
    };
    Patch patches[64]{};
    size_t patch_count = 0;
    // What slot 13 holds while the hooks are in place. One comparison against it says whether the
    // runtime has rewritten the family since the last look.
    void* sentinel_replacement = nullptr;
    std::atomic<uint32_t> vtable_refreshes{0};
    // Written once during install and never again, so read without the lock: the interesting log
    // lines are emitted with none held.
    rsf_frame_tap_options options{};
    // The caller's role rules, copied at install so the table need not outlive the call.
    rsf_role_rule role_rules[RSF_FRAME_TAP_MAX_ROLE_RULES]{};
    uint32_t role_count = 0;
    std::atomic<rsf_frame_tap_target_fn> research_draw{nullptr};
    std::atomic<rsf_frame_tap_target_fn> research_before{nullptr};
    std::atomic<rsf_frame_tap_compute_fn> research_compute{nullptr};
    std::atomic<void*> research_user{nullptr};
    ID3D11DeviceContext* observed_context = nullptr;
    void* input_watch = nullptr;
    bool input_watch_dirty = true;
    bool input_watch_bound = false;
    bool depth_bound = false;
    uint32_t target_count = 0;

    // The most recent pixel stage constant buffer of the watched size. Held with a reference so it
    // survives being unbound before a callback reads it. Only kept when `on_pass` can read it.
    ID3D11Buffer* view_constants = nullptr;
    // What `view_constants` holds, for the bind hook to compare against without taking the lock.
    // Never dereferenced.
    std::atomic<ID3D11Buffer*> view_constants_hint{nullptr};

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
    // Which slots hold a view, one bit each, so a walk visits the occupied few of the 128 and not
    // all of them.
    uint64_t occupied[max_examined_views / 64]{};
    // Whether the shadow is kept at all. Nothing reads it unless a pass, a watch report, a candidate
    // prefilter, a divert, a constant override or a research capture is wired up, and those are
    // known to `refresh_shadow_need`. `shadow_live` is what the render thread last acted on: when
    // the need appears the shadow is seeded from the context, and when it goes the references drop.
    std::atomic<bool> shadow_needed{false};
    bool shadow_live = false;
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
    // The bound view's format, which for a typeless texture is the one that means anything.
    uint32_t target_view_format = 0;
    // Draws into the current target since it was bound. Reset by a change of binding, so it counts
    // a pass rather than a frame.
    uint32_t draws_into_target = 0;

    // The last three pieces of pipeline state a draw is identified by. The input layout, the vertex
    // shader, the strides and the topology are already shadowed in `geometry`; these are what the
    // classifier needs and nothing here had.
    //
    // None is retained. Each is exactly what the game currently has bound, and the game keeps its
    // own bound state alive; a pointer here is only ever compared, never dereferenced. That also
    // means a pointer is only meaningful while it is bound, which is why identity is settled at
    // creation and looked up here rather than the other way round.
    ID3D11PixelShader* pixel_shader = nullptr;
    ID3D11BlendState* blend_state = nullptr;
    ID3D11DepthStencilState* depth_stencil_state = nullptr;

    // The candidate sets, copied so the caller may rebuild its own storage. Counts are atomic and
    // published after the entries are written, so the render thread either sees an old set or a
    // complete new one. A newly created layout being missed for a few draws is the worst case, and
    // it corrects itself on the next draw; the alternative is a lock on every draw in the frame.
    static constexpr uint32_t max_candidates = 64;
    void* candidate_layouts[max_candidates]{};
    void* candidate_widget_targets[max_candidates]{};
    void* candidate_shaders[max_candidates]{};
    std::atomic<uint32_t> candidate_layout_count{0};
    std::atomic<uint32_t> candidate_widget_target_count{0};
    std::atomic<uint32_t> candidate_shader_count{0};
    // One load rejects the whole mechanism when nothing has been named, which is what the frame
    // pays before anything is set up.
    std::atomic<uint32_t> candidates_armed{0};
    std::atomic<uint64_t> candidate_draws{0};

    // Diverting. Armed separately from the candidate sets, so classification can run for a whole
    // milestone with nothing moved.
    std::atomic<uint32_t> divert_armed{0};
    // The constant override: asked before a candidate draw, swaps constant buffers for that draw
    // only.
    std::atomic<rsf_frame_tap_constant_override_fn> constant_override{nullptr};
    std::atomic<void*> constant_override_user{nullptr};
    std::atomic<uint32_t> constant_override_format{0};
    std::atomic<uint32_t> draws_overridden{0};
    std::atomic<uint32_t> copies_redirected{0};
    std::atomic<uint32_t> copies_mismatched{0};
    struct OverrideState {
        bool active = false;
        uint32_t count = 0;
        UINT slots[RSF_FRAME_TAP_CONSTANT_SLOTS] = {};
        ID3D11Buffer* originals[RSF_FRAME_TAP_CONSTANT_SLOTS] = {};
    } override_state;
    // The constant watch: buffers of one width mapped for writing, held until their Unmap.
    std::atomic<rsf_frame_tap_constants_fn> constant_watch{nullptr};
    std::atomic<void*> constant_watch_user{nullptr};
    std::atomic<uint32_t> constant_watch_bytes{0};
    // Whether the watch may write into what it is handed. When it may not, an UpdateSubresource
    // upload is handed over as the game's own memory instead of a scratch copy.
    std::atomic<bool> constant_watch_writable{true};
    // Whether a resource is a constant buffer the watch wants, and its width, remembered per
    // address so an upload pays two virtual calls once and not on every call. Render thread only.
    // An address can be reused by another resource, so the table is emptied whenever the epoch
    // moves (each frame, and when the watch changes) and after a fixed number of lookups.
    struct BufferVerdict {
        const void* resource = nullptr;
        uint32_t bytes = 0;  // zero: not a buffer the watch wants
    };
    static constexpr uint32_t verdict_slots = 64;
    static constexpr uint32_t verdict_lifetime = 16384;
    BufferVerdict verdicts[verdict_slots]{};
    uint32_t verdict_lookups = 0;
    uint32_t verdict_epoch_seen = 0;
    std::atomic<uint32_t> verdict_epoch{0};
    std::atomic<uint32_t> updates_watched{0};
    std::atomic<uint32_t> gates_declined{0};
    struct PendingMap {
        ID3D11Resource* resource = nullptr;
        void* data = nullptr;
        uint32_t bytes = 0;
    };
    PendingMap pending_maps[16]{};
    ID3D11RenderTargetView* layer_target = nullptr;
    ID3D11Texture2D* layer_texture = nullptr;
    uint32_t layer_width = 0;
    uint32_t layer_height = 0;
    rsf_frame_tap_verdict_fn verdict = nullptr;
    void* verdict_user = nullptr;
    std::atomic<uint64_t> draws_diverted{0};
    std::atomic<uint64_t> blend_states_patched{0};
    std::atomic<uint64_t> divert_refused{0};
    std::atomic<uint32_t> divert_last_refusal{0};

    // Blend states with their alpha operations patched, keyed by the state the game bound.
    //
    // Keyed by pointer, which is only safe because a blend state that is released takes its entry
    // with it: `forget_blend` is called from the release path. The alternative, rebuilding the
    // patched state per draw, would create a device object inside a draw hook, which is the one
    // place it must not happen.
    struct PatchedBlend {
        ID3D11BlendState* original;
        ID3D11BlendState* patched;
    };
    static constexpr uint32_t max_patched_blends = 32;
    PatchedBlend patched_blends[max_patched_blends]{};
    uint32_t patched_blend_count = 0;

    // What the divert replaced, restored after the draw is forwarded. Only ever written and read on
    // the render thread between a draw's begin and end, so it needs no synchronisation.
    struct DivertState {
        bool active = false;
        ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT] = {};
        ID3D11DepthStencilView* depth_view = nullptr;
        ID3D11BlendState* blend = nullptr;
        FLOAT blend_factor[4] = {};
        UINT blend_mask = 0;
        UINT viewport_count = 0;
        D3D11_VIEWPORT viewports[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE] = {};
        UINT scissor_count = 0;
        D3D11_RECT scissors[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE] = {};
    };
    DivertState divert;

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
    // Substituted bindings whose depth was the wrong size for the promoted target. The last pair is
    // kept alongside the count so a report can name it, since "some mismatched" and "a 1024x576
    // depth met a 2048x1152 target" are answers of very different use.
    std::atomic<uint32_t> depth_mismatches{0};
    std::atomic<uint32_t> depth_mismatch_target_width{0};
    std::atomic<uint32_t> depth_mismatch_target_height{0};
    std::atomic<uint32_t> depth_mismatch_depth_width{0};
    std::atomic<uint32_t> depth_mismatch_depth_height{0};
    std::atomic<uint32_t> depth_mismatch_depth_format{0};

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
// A depth rather than a flag.
//
// Most hooks check this and return before constructing a guard, so a nested call that bails out
// leaves it alone and a flag survives. The substitution paths are the exception: their guard is
// constructed before that check, because the swap has to happen whether or not the call is
// recognised. So a callback that binds something while a plan is active constructs and destroys a
// guard inside an outer one, and with a flag the destructor clears it, leaving the rest of the
// outer hook recognising bindings that belong to the callback rather than to the game.
//
// Nothing has gone wrong from it yet, because the callbacks that run today bind nothing. That
// stops being true as soon as a hook issues context calls of its own, which is exactly what
// diverting a draw is. Counting costs the same and does not depend on which of two orderings a
// given hook happens to use.
// It reads as a flag at every use, which is why it keeps the name: zero is outside, anything else
// is inside, and every existing `if (inside_hook || ...)` means what it did before.
thread_local uint32_t inside_hook = 0;

struct ReentryGuard {
    ReentryGuard() { ++inside_hook; }
    ~ReentryGuard() { --inside_hook; }
    ReentryGuard(const ReentryGuard&) = delete;
    ReentryGuard& operator=(const ReentryGuard&) = delete;
};

// The runtime rewrites its own vtable, and the hooks have to survive that.
//
// Measured on Windows 11 with the stock d3d11.dll on 26 September 2026, with a scratch probe that
// snapshotted the table after every call: the immediate context's vtable lives on the heap, and the
// runtime rewrites the whole work-submission family of entries, slots 12, 13, 20, 21, 38 to 42,
// 46 to 54, 57, 115 and 116, whenever a flush-class call runs (a Map for reading, Flush) and again
// on the next draw, dispatch, copy or clear, flipping between two sets of implementations. Each
// rewrite discards whatever was patched into those slots. DXVK's table is static and never
// rewritten, which is why every Wine run of this module passed and no Windows run observed a draw
// after the first Map.
//
// So every hook checks one sentinel entry on the way in and re-applies the whole table when it is
// gone, recording what the runtime had written as the new original. That is the right thing to
// forward to: whatever variant is in the slot is the one for the runtime's current state, and the
// runtime rewrites the slot again, removing the hook, before that state changes. The flush-class
// calls and every member of the family are hooked as well, as pass-throughs where nothing needed
// to observe them, so the flip that happens inside a call is noticed by that call's own epilogue
// rather than by the next binding. The one flip nothing here sees is the one Present causes, and
// `rsf_frame_tap_refresh` exists for the present hook to call.
void refresh_hooks_slow(Tap& self)
{
    uint32_t rewritten = 0;
    for (size_t index = 0; index < self.patch_count; ++index) {
        Tap::Patch& patch = self.patches[index];
        if (self.vtable[patch.slot] == patch.replacement) {
            continue;
        }
        if (rsf::patch_slot(self.vtable, patch.slot, patch.replacement, patch.original)) {
            ++rewritten;
        }
    }
    if (rewritten) {
        self.vtable_refreshes.fetch_add(1, std::memory_order_relaxed);
    }
}

inline void refresh_hooks(Tap& self)
{
    if (!self.installed || !self.vtable ||
        self.vtable[slot_draw] == self.sentinel_replacement) {
        return;
    }
    refresh_hooks_slow(self);
}

// What every hook goes through on the way in. The tap, after the table has been checked.
inline Tap& enter_hook()
{
    Tap& self = tap();
    refresh_hooks(self);
    return self;
}

// The texture behind a view, with a reference the caller owns. Both GetResource and QueryInterface
// hand out references, so the intermediate one is dropped here rather than leaked per binding. Shader
// resource and render target views share ID3D11View, which is where GetResource lives.
ID3D11Texture2D* texture_behind(ID3D11View* view)
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

// The extent and format of whatever a view is looking at. False when it is not a 2D texture, which
// leaves the caller to treat the size as unknown rather than as zero.
bool view_extent(ID3D11View* view, uint32_t& width, uint32_t& height, uint32_t& format)
{
    ID3D11Texture2D* texture = view ? texture_behind(view) : nullptr;
    if (!texture) {
        return false;
    }
    D3D11_TEXTURE2D_DESC description{};
    texture->GetDesc(&description);
    texture->Release();
    width = description.Width;
    height = description.Height;
    format = description.Format;
    return true;
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

// The plan entry that substitutes this texture right now with the given kind of view
// (`shader_view` or `render_view`), or null: named by the plan, carrying that view, and past its
// gate.
const rsf_frame_tap_substitution* applicable_entry(const Tap& self, void* texture,
                                                   void* rsf_frame_tap_substitution::*view)
{
    uint32_t index = 0;
    const rsf_frame_tap_substitution* entry = find_entry(self, texture, index);
    if (!entry || !(entry->*view) || !entry_applies(self, index)) {
        return nullptr;
    }
    return entry;
}

// Every gate shut, so the next frame opens them again.
void close_gates(Tap& self)
{
    for (bool& gate : self.gate_open) {
        gate = false;
    }
}

// The stand-in texture for a promoted resource whose substitution applies now, with a reference
// the caller releases, or null.
ID3D11Resource* promoted_resource(const Tap& self, ID3D11Resource* resource)
{
    if (!resource) {
        return nullptr;
    }
    const rsf_frame_tap_substitution* entry =
        applicable_entry(self, resource, &rsf_frame_tap_substitution::render_view);
    if (!entry) {
        return nullptr;
    }
    ID3D11Resource* stand_in = nullptr;
    static_cast<ID3D11RenderTargetView*>(entry->render_view)->GetResource(&stand_in);
    return stand_in;
}

// Open any gate this render target opens, and tell the caller. Runs before the binding is
// forwarded, which is the whole point: the gate is the moment the scene is finished and nothing
// downstream has read it, and a reconstruction that ran afterwards would be a frame late.
void open_gates_for(Tap& self, ID3D11DeviceContext* context, void* texture)
{
    if (!texture) {
        return;
    }
    bool asked = false, accepted = true;
    for (uint32_t index = 0; index < self.plan.count; ++index) {
        if (self.plan.items[index].after_target != texture || self.gate_open[index]) {
            continue;
        }
        if (self.plan.on_gate && !asked) {
            // Under the reentry guard the caller set up, so whatever this binds comes back through
            // these hooks as the tap's own work rather than as the game's. A callback that declines
            // leaves the gate shut for the next binding of this target.
            accepted = self.plan.on_gate(self.plan.on_gate_user, context, texture) != 0;
            asked = true;
            if (!accepted) {
                self.gates_declined.fetch_add(1, std::memory_order_relaxed);
            }
        }
        if (!accepted) continue;
        self.gate_open[index] = true;
        self.gates_opened.fetch_add(1, std::memory_order_relaxed);
    }
}

// The viewports and scissor rectangles scaled, for a target of another extent than the game thinks
// it is drawing into.
void scale_viewports(const D3D11_VIEWPORT* source, UINT count, float scale_x, float scale_y,
                     D3D11_VIEWPORT* scaled)
{
    for (UINT index = 0; index < count; ++index) {
        scaled[index] = source[index];
        scaled[index].TopLeftX *= scale_x;
        scaled[index].TopLeftY *= scale_y;
        scaled[index].Width *= scale_x;
        scaled[index].Height *= scale_y;
    }
}

void scale_scissors(const D3D11_RECT* source, UINT count, float scale_x, float scale_y,
                    D3D11_RECT* scaled)
{
    for (UINT index = 0; index < count; ++index) {
        scaled[index].left = LONG(float(source[index].left) * scale_x);
        scaled[index].top = LONG(float(source[index].top) * scale_y);
        scaled[index].right = LONG(float(source[index].right) * scale_x);
        scaled[index].bottom = LONG(float(source[index].bottom) * scale_y);
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
        scale_viewports(self.game_viewports, self.game_viewport_count, scale_x, scale_y, scaled);
        self.original_set_viewports(context, self.game_viewport_count, scaled);
    }
    if (self.game_scissor_count > 0 && self.original_set_scissors) {
        D3D11_RECT scaled[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE];
        scale_scissors(self.game_scissors, self.game_scissor_count, scale_x, scale_y, scaled);
        self.original_set_scissors(context, self.game_scissor_count, scaled);
    }
}

// Slot bookkeeping. A slot holding a view has its bit set; the texture behind it is held with a
// reference of its own when it is a 2D texture.
inline void set_occupied(Tap& self, UINT index, bool occupied)
{
    const uint64_t bit = uint64_t(1) << (index % 64);
    if (occupied) {
        self.occupied[index / 64] |= bit;
    } else {
        self.occupied[index / 64] &= ~bit;
    }
}

// Empty a slot, dropping the texture it held.
void drop_slot(Tap& self, UINT index)
{
    Tap::Slot& slot = self.slots[index];
    if (slot.texture) {
        slot.texture->Release();
    }
    slot = Tap::Slot{};
    set_occupied(self, index, false);
}

// Call `visit(index)` for each slot holding a view, lowest first. The visit may drop slots.
template <class Visit>
void each_occupied(const Tap& self, Visit&& visit)
{
    for (size_t word = 0; word < std::size(self.occupied); ++word) {
        uint64_t bits = self.occupied[word];
        while (bits) {
            const UINT index = UINT(word * 64 + std::countr_zero(bits));
            bits &= bits - 1;
            visit(index);
        }
    }
}

// Record what the output merger now has at render target slot 0. Called from both binding hooks,
// with the game's call already forwarded. `resolved` is a texture reference the caller already took
// for this view, if it did; it is adopted or released here.
void shadow_render_target(Tap& self, ID3D11RenderTargetView* view,
                          ID3D11Texture2D* resolved = nullptr)
{
    if (self.target_view == view) {
        // The same view rebound is most of what a frame does. It is not a new pass, so the draw
        // ordinal deliberately continues rather than restarting: a pass that rebinds its own
        // target between draws would otherwise report every draw as the first one.
        if (resolved) {
            resolved->Release();
        }
        return;
    }
    if (self.target_texture) {
        self.target_texture->Release();
        self.target_texture = nullptr;
    }
    self.target_description = D3D11_TEXTURE2D_DESC{};
    self.target_view = view;
    self.draws_into_target = 0;
    self.target_view_format = 0;
    if (!view) {
        return;
    }
    self.target_texture = resolved ? resolved : texture_behind(view);
    if (self.target_texture) {
        self.target_texture->GetDesc(&self.target_description);
    }
    /* The view's format as well as the texture's, because for a typeless texture they differ and it
       is the view that decides what a shader's output means on the way in. Unreal allocates its
       targets typeless and picks sRGB or not per view, so the texture format cannot answer whether
       a draw's colour is being encoded, and a layer that does not encode where the original did
       stores linear values that later read as too dark. */
    D3D11_RENDER_TARGET_VIEW_DESC view_description{};
    view->GetDesc(&view_description);
    self.target_view_format = static_cast<uint32_t>(view_description.Format);
}

// AC7 post passes use output slot zero. Clear its implicit read/write hazard from the shadow.
void unbind_target_reads(Tap& self)
{
    if (!self.target_texture) {
        return;
    }
    each_occupied(self, [&](UINT index) {
        if (self.slots[index].texture == self.target_texture) {
            drop_slot(self, index);
            self.shadow_dirty = true;
            self.input_watch_dirty = true;
        }
    });
}

/* Is this draw one of the handful in the frame worth a second look?
 *
 * Pointer comparisons over state already shadowed, guarded by a load that rejects everything until
 * something has been named. This is what the rest of the frame pays, so it touches no memory the
 * draw path had not already touched and calls nothing. */
bool candidate_passes(const Tap& self)
{
    if (self.candidates_armed.load(std::memory_order_relaxed) == 0) {
        return false;
    }
    const uint32_t layouts = self.candidate_layout_count.load(std::memory_order_acquire);
    for (uint32_t index = 0; index < layouts; ++index) {
        if (self.geometry.input_layout == self.candidate_layouts[index]) {
            return true;
        }
    }
    const uint32_t shaders = self.candidate_shader_count.load(std::memory_order_acquire);
    for (uint32_t index = 0; index < shaders; ++index) {
        if (self.pixel_shader == self.candidate_shaders[index] ||
            self.geometry.vertex_shader == self.candidate_shaders[index]) {
            return true;
        }
    }
    const uint32_t targets = self.candidate_widget_target_count.load(std::memory_order_acquire);
    for (uint32_t slot = 0; slot < RSF_FRAME_TAP_CANDIDATE_SLOTS && targets != 0; ++slot) {
        ID3D11Texture2D* texture = self.slots[slot].texture;
        if (!texture) {
            continue;
        }
        for (uint32_t index = 0; index < targets; ++index) {
            if (texture == self.candidate_widget_targets[index]) {
                return true;
            }
        }
    }
    return false;
}

/* What the shadow knows of a draw, for the reports made after it and for the questions asked
 * before it. The inputs and the constant buffers are the caller's to add. */
void fill_common_facts(const Tap& self, bool indexed, UINT element_count,
                       rsf_frame_tap_target_draw& facts)
{
    facts = rsf_frame_tap_target_draw{};
    facts.struct_size = sizeof(facts);
    facts.watch_index = RSF_FRAME_TAP_WATCH_SLOTS;
    facts.render_target = self.target_texture;
    facts.target_width = self.target_description.Width;
    facts.target_height = self.target_description.Height;
    facts.target_format = static_cast<uint32_t>(self.target_description.Format);
    facts.target_view_format = self.target_view_format;
    facts.target_count = self.target_count;
    facts.target_samples = self.target_description.SampleDesc.Count;
    facts.depth_bound = self.depth_bound ? 1u : 0u;
    facts.indexed = indexed ? 1u : 0u;
    facts.element_count = element_count;
    facts.pixel_shader = self.pixel_shader;
    facts.vertex_shader = self.geometry.vertex_shader;
    facts.input_layout = self.geometry.input_layout;
    facts.blend_state = self.blend_state;
    facts.depth_stencil_state = self.depth_stencil_state;
    facts.vertex_stride = self.geometry.strides[0];
    facts.topology = static_cast<uint32_t>(self.geometry.topology);
}

/* The facts a verdict is decided from, filled from the shadow before the draw is forwarded.
 *
 * Deliberately not the full report `consider_target_draw` builds: that one asks the context for the
 * viewport, which is a call this cannot afford on a path that runs before every candidate draw.
 * What the classifier needs is what the shadow already holds. */
void fill_divert_facts(const Tap& self, bool indexed, UINT element_count,
                       rsf_frame_tap_target_draw& facts, rsf_frame_tap_input* inputs)
{
    fill_common_facts(self, indexed, element_count, facts);
    for (int slot = 0; slot < 14; ++slot) {
        facts.vertex_constants[slot] = self.geometry.vertex_constants[slot];
        facts.pixel_constants[slot] = self.pixel_constants[slot];
    }

    uint32_t used = 0;
    each_occupied(self, [&](UINT slot) {
        const Tap::Slot& entry = self.slots[slot];
        if (!entry.texture || used == RSF_FRAME_TAP_MAX_INPUTS) {
            return;
        }
        inputs[used].slot = slot;
        inputs[used].texture = entry.texture;
        inputs[used].width = entry.description.Width;
        inputs[used].height = entry.description.Height;
        inputs[used].format = static_cast<uint32_t>(entry.description.Format);
        ++used;
    });
    facts.input_count = used;
    facts.inputs = inputs;
}

void refuse_divert(Tap& self, uint32_t reason)
{
    self.divert_refused.fetch_add(1, std::memory_order_relaxed);
    self.divert_last_refusal.store(reason, std::memory_order_relaxed);
}

/* The game's blend with its alpha operations replaced, created once and cached.
 *
 * Colour factors are copied unchanged. Only `SrcBlendAlpha`, `DestBlendAlpha` and `BlendOpAlpha`
 * move, to `One / InvSrcAlpha / Add`, which is the over operator on coverage: a1 + a2(1 - a1).
 * Everything else about the draw, including which channels it writes, is the game's.
 *
 * Returns null when the state cannot be built, and the caller then refuses the divert rather than
 * moving a draw whose coverage would be lost. */
ID3D11BlendState* patched_blend_for(Tap& self, ID3D11DeviceContext* context,
                                    ID3D11BlendState* original)
{
    for (uint32_t index = 0; index < self.patched_blend_count; ++index) {
        if (self.patched_blends[index].original == original) {
            return self.patched_blends[index].patched;
        }
    }
    if (self.patched_blend_count >= Tap::max_patched_blends) {
        return nullptr;
    }

    D3D11_BLEND_DESC description{};
    if (original) {
        original->GetDesc(&description);
    } else {
        // No blend state bound is D3D11's default: blending off, all channels written. Diverting
        // such a draw still needs alpha, so the default is spelled out and then patched like any
        // other, rather than treated as a case with no answer.
        description.RenderTarget[0].BlendEnable = FALSE;
        description.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    }
    const UINT targets = description.IndependentBlendEnable ? 8u : 1u;
    for (UINT index = 0; index < targets; ++index) {
        D3D11_RENDER_TARGET_BLEND_DESC& target = description.RenderTarget[index];
        // The alpha channel has to be writable before any of the operations below mean anything.
        //
        // A blend that writes colour only is ordinary for an interface drawn into a target whose
        // alpha nobody reads, and AC7's is exactly that. Patching the alpha operations of such a
        // state changes nothing at all: the channel is masked off, so the layer accumulates colour
        // and no coverage, and a premultiplied composite of colour with zero coverage contributes
        // nothing. Which is a black intro image, and a menu that is dark and washed out in
        // proportion to how much coverage it was missing.
        target.RenderTargetWriteMask |= D3D11_COLOR_WRITE_ENABLE_ALPHA;
        if (!target.BlendEnable) {
            // An opaque draw already writes alpha 1 where it covers, which is the coverage a layer
            // wants. Nothing further to patch, and enabling a blend here would change the colour.
            continue;
        }
        target.SrcBlendAlpha = D3D11_BLEND_ONE;
        target.DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
        target.BlendOpAlpha = D3D11_BLEND_OP_ADD;
    }

    ID3D11Device* device = nullptr;
    context->GetDevice(&device);
    if (!device) {
        return nullptr;
    }
    ID3D11BlendState* patched = nullptr;
    const HRESULT made = device->CreateBlendState(&description, &patched);
    device->Release();
    if (FAILED(made) || !patched) {
        return nullptr;
    }
    self.patched_blends[self.patched_blend_count].original = original;
    self.patched_blends[self.patched_blend_count].patched = patched;
    ++self.patched_blend_count;
    self.blend_states_patched.fetch_add(1, std::memory_order_relaxed);

    /* What the game's blend was, once per distinct state. There are only ever a handful, and this
       is the line that says whether a layer with no coverage is the blend's fault: a write mask
       without its alpha bit makes every alpha operation below it decorative. */
    D3D11_BLEND_DESC before{};
    if (original) {
        original->GetDesc(&before);
    }
    rsf::say(self.options.log, self.options.log_user,
             "divert: patched a blend. Before: enable %u, colour %u/%u op %u, alpha %u/%u op %u, "
             "write mask 0x%x. After: alpha 2/6 op 1, write mask 0x%x",
             before.RenderTarget[0].BlendEnable, before.RenderTarget[0].SrcBlend,
             before.RenderTarget[0].DestBlend, before.RenderTarget[0].BlendOp,
             before.RenderTarget[0].SrcBlendAlpha, before.RenderTarget[0].DestBlendAlpha,
             before.RenderTarget[0].BlendOpAlpha, before.RenderTarget[0].RenderTargetWriteMask,
             description.RenderTarget[0].RenderTargetWriteMask);
    return patched;
}

// Drop the references a divert took when it saved the context's state, and forget the state.
void release_saved(Tap::DivertState& divert)
{
    for (auto* target : divert.targets) {
        if (target) {
            target->Release();
        }
    }
    if (divert.depth_view) {
        divert.depth_view->Release();
    }
    if (divert.blend) {
        divert.blend->Release();
    }
    divert = Tap::DivertState{};
}

/* Move this draw to the layer, if the caller says it is the interface and nothing makes that
   unsafe. Returns true when the state below has to be put back afterwards. */
bool begin_divert(Tap& self, ID3D11DeviceContext* context,
                  const rsf_frame_tap_target_draw& facts)
{
    if (self.divert_armed.load(std::memory_order_relaxed) == 0 || !self.verdict) {
        return false;
    }
    const rsf_frame_tap_verdict verdict = self.verdict(self.verdict_user, &facts);
    if (verdict == RSF_FRAME_TAP_LEAVE) {
        return false;
    }
    if (!self.layer_target) {
        refuse_divert(self, RSF_FRAME_TAP_REFUSED_NO_LAYER);
        return false;
    }
    if (self.target_count != 1) {
        refuse_divert(self, RSF_FRAME_TAP_REFUSED_MULTIPLE_TARGETS);
        return false;
    }
    if (self.target_texture && self.target_texture == self.layer_texture) {
        refuse_divert(self, RSF_FRAME_TAP_REFUSED_ALREADY_LAYER);
        return false;
    }

    Tap::DivertState& divert = self.divert;
    divert = Tap::DivertState{};

    // Saved from the context rather than from the shadow, because what has to be put back is
    // exactly what was bound, and the shadow deliberately holds slot zero only.
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, divert.targets,
                               &divert.depth_view);
    context->OMGetBlendState(&divert.blend, divert.blend_factor, &divert.blend_mask);
    divert.viewport_count = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
    context->RSGetViewports(&divert.viewport_count, divert.viewports);
    divert.scissor_count = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
    context->RSGetScissorRects(&divert.scissor_count, divert.scissors);

    ID3D11BlendState* blend = divert.blend;
    if (verdict == RSF_FRAME_TAP_DIVERT_PATCH_ALPHA) {
        blend = patched_blend_for(self, context, divert.blend);
        if (!blend) {
            // Put back what was taken and leave the draw where it was. A layer with colour and no
            // coverage composites to nothing, so moving it would lose the interface outright.
            release_saved(divert);
            refuse_divert(self, RSF_FRAME_TAP_REFUSED_BLEND);
            return false;
        }
    }

    // No depth stencil view. The interface is an overlay on the layer, and the scene's depth
    // belongs to a target of another extent that this draw no longer writes.
    ID3D11RenderTargetView* layer = self.layer_target;
    self.original_set_targets(context, 1, &layer, nullptr);
    context->OMSetBlendState(blend, divert.blend_factor, divert.blend_mask);

    // The viewport scaled by what the draw covered of its own target. A quad drawn into a
    // render-resolution layer covers the same fraction of the frame as it will of ours, so the
    // fraction is preserved rather than the pixel count.
    if (divert.viewport_count >= 1 && self.target_description.Width != 0 &&
        self.target_description.Height != 0 && self.layer_width != 0 && self.layer_height != 0) {
        const float scale_x =
            static_cast<float>(self.layer_width) / static_cast<float>(self.target_description.Width);
        const float scale_y = static_cast<float>(self.layer_height) /
                              static_cast<float>(self.target_description.Height);
        D3D11_VIEWPORT scaled[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE];
        scale_viewports(divert.viewports, divert.viewport_count, scale_x, scale_y, scaled);
        context->RSSetViewports(divert.viewport_count, scaled);

        if (divert.scissor_count >= 1) {
            D3D11_RECT scissors[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE];
            scale_scissors(divert.scissors, divert.scissor_count, scale_x, scale_y, scissors);
            context->RSSetScissorRects(divert.scissor_count, scissors);
        }
    }

    divert.active = true;
    self.draws_diverted.fetch_add(1, std::memory_order_relaxed);
    return true;
}

void seed_geometry(Tap& self, ID3D11DeviceContext* context);

// Query the less common stages only on an offered draw. The context owns these bindings;
// release the getter references immediately and restore every overridden slot after drawing.
void extra_stage_constants(ID3D11DeviceContext* context, rsf_frame_tap_target_draw& facts)
{
    ID3D11Buffer* buffers[14]{};
    auto copy = [&](void** target) {
        for (UINT i = 0; i < 14; ++i) {
            target[i] = buffers[i];
            if (buffers[i]) { buffers[i]->Release(); }
        }
    };
    context->GSGetConstantBuffers(0, 14, buffers); copy(facts.geometry_constants);
    context->HSGetConstantBuffers(0, 14, buffers); copy(facts.hull_constants);
    context->DSGetConstantBuffers(0, 14, buffers); copy(facts.domain_constants);
}

void bind_override(Tap& self, ID3D11DeviceContext* context, UINT encoded_slot, ID3D11Buffer* buffer)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, ID3D11Buffer* const*);
    const UINT slot = encoded_slot % 14;
    switch (encoded_slot / 14) {
    case 0: reinterpret_cast<Fn>(self.extra_originals[5])(context, slot, 1, &buffer); break;
    case 1: self.original_set_constants(context, slot, 1, &buffer); break;
    case 2: context->GSSetConstantBuffers(slot, 1, &buffer); break;
    case 3: context->HSSetConstantBuffers(slot, 1, &buffer); break;
    case 4: context->DSSetConstantBuffers(slot, 1, &buffer); break;
    }
}

// Bind stand-in constant buffers for a candidate draw, if the override callback asks. See
// `rsf_frame_tap_set_constant_override`. Called after try_divert declined, with the guard held.
void begin_constant_override(Tap& self, ID3D11DeviceContext* context, bool indexed, UINT element_count)
{
    Tap::OverrideState& state = self.override_state;
    state.active = false;
    const rsf_frame_tap_constant_override_fn fn =
        self.constant_override.load(std::memory_order_acquire);
    if (!fn || !self.extra_originals[5]) {
        return;
    }
    if (!self.geometry_valid) {
        seed_geometry(self, context);
    }
    const uint32_t offered_format = self.constant_override_format.load(std::memory_order_relaxed);
    const bool offered = self.target_texture && offered_format != 0 &&
        offered_format == static_cast<uint32_t>(self.target_description.Format);
    if (!offered && !candidate_passes(self)) {
        return;
    }
    rsf_frame_tap_target_draw facts;
    rsf_frame_tap_input inputs[RSF_FRAME_TAP_MAX_INPUTS];
    fill_divert_facts(self, indexed, element_count, facts, inputs);
    extra_stage_constants(context, facts);
    void* const* bound[] = {facts.vertex_constants, facts.pixel_constants, facts.geometry_constants,
                           facts.hull_constants, facts.domain_constants};
    uint32_t slots[RSF_FRAME_TAP_CONSTANT_SLOTS] = {};
    void* buffers[RSF_FRAME_TAP_CONSTANT_SLOTS] = {};
    const int asked =
        fn(self.constant_override_user.load(std::memory_order_relaxed), &facts, slots, buffers);
    if (asked <= 0) {
        return;
    }
    state.count = 0;
    bool used[RSF_FRAME_TAP_CONSTANT_SLOTS] = {};
    for (int index = 0; index < asked && index < int(RSF_FRAME_TAP_CONSTANT_SLOTS); ++index) {
        if (!buffers[index] || slots[index] >= RSF_FRAME_TAP_CONSTANT_SLOTS || used[slots[index]]) {
            continue;
        }
        used[slots[index]] = true;
        auto* replacement = static_cast<ID3D11Buffer*>(buffers[index]);
        bind_override(self, context, slots[index], replacement);
        state.slots[state.count] = slots[index];
        state.originals[state.count] =
            static_cast<ID3D11Buffer*>(bound[slots[index] / 14][slots[index] % 14]);
        ++state.count;
    }
    if (state.count == 0) {
        return;
    }
    state.active = true;
    self.draws_overridden.fetch_add(1, std::memory_order_relaxed);
}

void end_constant_override(Tap& self, ID3D11DeviceContext* context)
{
    Tap::OverrideState& state = self.override_state;
    if (!state.active) {
        return;
    }
    for (uint32_t index = 0; index < state.count; ++index) {
        bind_override(self, context, state.slots[index], state.originals[index]);
    }
    state.count = 0;
    state.active = false;
}

bool try_divert(Tap& self, ID3D11DeviceContext* context, bool indexed, UINT element_count)
{
    if (self.divert_armed.load(std::memory_order_relaxed) == 0 || !candidate_passes(self)) {
        return false;
    }
    rsf_frame_tap_target_draw facts;
    rsf_frame_tap_input inputs[RSF_FRAME_TAP_MAX_INPUTS];
    fill_divert_facts(self, indexed, element_count, facts, inputs);
    return begin_divert(self, context, facts);
}

void end_divert(Tap& self, ID3D11DeviceContext* context)
{
    Tap::DivertState& divert = self.divert;
    if (!divert.active) {
        return;
    }
    self.original_set_targets(context, D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, divert.targets,
                              divert.depth_view);
    context->OMSetBlendState(divert.blend, divert.blend_factor, divert.blend_mask);
    if (divert.viewport_count >= 1) {
        context->RSSetViewports(divert.viewport_count, divert.viewports);
    }
    if (divert.scissor_count >= 1) {
        context->RSSetScissorRects(divert.scissor_count, divert.scissors);
    }
    release_saved(divert);
}

void consider_target_draw(Tap& self, ID3D11DeviceContext* context, bool indexed,
                          UINT element_count, bool before = false)
{
    const auto research = before ? self.research_before.load(std::memory_order_acquire) :
        self.research_draw.load(std::memory_order_acquire);
    if (before && (!research || context != self.observed_context)) return;
    if (!self.target_texture) {
        return;
    }
    const uint32_t ordinal = before ? self.draws_into_target : self.draws_into_target++;

    uint32_t watch_index = RSF_FRAME_TAP_WATCH_SLOTS;
    for (uint32_t index = 0; index < RSF_FRAME_TAP_WATCH_SLOTS; ++index) {
        if (self.watch[index].load(std::memory_order_relaxed) == self.target_texture) {
            watch_index = index;
            break;
        }
    }
    if (self.input_watch_dirty) {
        self.input_watch_bound = false;
        if (self.input_watch) {
            each_occupied(self, [&](UINT index) {
                if (self.slots[index].texture == self.input_watch) {
                    self.input_watch_bound = true;
                }
            });
        }
        self.input_watch_dirty = false;
    }
    const bool report_input = !before && self.options.on_input_draw && self.input_watch_bound &&
                              context == self.observed_context;

    // Is this one of the handful of draws in the frame worth describing? Pointer comparisons over
    // the shadow, guarded by a single load that rejects everything until something has been named.
    // The sets are small by construction, so these loops are a few compares and no memory the draw
    // path did not already touch.
    bool report_candidate = false;
    if (!before && self.options.on_candidate_draw && context == self.observed_context &&
        candidate_passes(self)) {
        report_candidate = true;
        self.candidate_draws.fetch_add(1, std::memory_order_relaxed);
    }

    bool report_target = false;
    if (!before && watch_index != RSF_FRAME_TAP_WATCH_SLOTS && self.options.on_target_draw) {
        uint32_t budget = self.watch_budget[watch_index].load(std::memory_order_relaxed);
        while (budget != 0) {
            if (budget == unlimited_budget ||
                self.watch_budget[watch_index].compare_exchange_weak(
                    budget, budget - 1, std::memory_order_relaxed)) {
                report_target = true;
                break;
            }
        }
    }
    const bool report_research = research && context == self.observed_context;
    if (!report_target && !report_input && !report_candidate && !report_research) {
        return;
    }

    rsf_frame_tap_input inputs[RSF_FRAME_TAP_MAX_INPUTS]{};
    uint32_t reported = 0;
    uint32_t bound = 0;
    each_occupied(self, [&](UINT index) {
        const Tap::Slot& entry = self.slots[index];
        ++bound;
        if (reported == RSF_FRAME_TAP_MAX_INPUTS) {
            return;
        }
        rsf_frame_tap_input& input = inputs[reported++];
        input.slot = index;
        input.texture = entry.texture;
        input.width = entry.description.Width;
        input.height = entry.description.Height;
        input.format = uint32_t(entry.description.Format);
    });

    rsf_frame_tap_target_draw report;
    fill_common_facts(self, indexed, element_count, report);
    report.context = context;
    report.watch_index = watch_index;
    report.draw_index = ordinal;
    report.input_count = reported;
    report.inputs = inputs;
    report.inputs_truncated = bound > reported ? 1u : 0u;

    // The viewport, asked for only on a draw that is being reported. It is the one thing here that
    // the shadow cannot supply, because nothing hooks RSSetViewports, and a call per reported draw
    // is affordable where a call per draw would not be.
    D3D11_VIEWPORT viewport{};
    UINT viewport_count = 1;
    context->RSGetViewports(&viewport_count, &viewport);
    if (viewport_count >= 1) {
        report.viewport_x = viewport.TopLeftX;
        report.viewport_y = viewport.TopLeftY;
        report.viewport_width = uint32_t(viewport.Width);
        report.viewport_height = uint32_t(viewport.Height);
    }

    // Not a count of what the caller was told: a report the caller ignores still happened, and a
    // watch that never fires is the thing this number exists to distinguish.
    if (report_target) {
        self.target_draws_reported.fetch_add(1, std::memory_order_relaxed);
        self.options.on_target_draw(self.options.on_target_draw_user, &report);
    }
    if (report_input) {
        report.watch_index = RSF_FRAME_TAP_WATCH_SLOTS;
        self.options.on_input_draw(self.options.on_input_draw_user, &report);
    }
    if (report_candidate) {
        // No watch slot: this draw was not reported because anyone named its target, but because
        // of what it is made of. Saying so keeps a candidate from being read as a watch hit.
        report.watch_index = RSF_FRAME_TAP_WATCH_SLOTS;
        self.options.on_candidate_draw(self.options.on_candidate_draw_user, &report);
    }
    if (report_research) {
        if (!self.geometry_valid) seed_geometry(self, context);
        report.vertex_shader = self.geometry.vertex_shader;
        report.input_layout = self.geometry.input_layout;
        report.vertex_stride = self.geometry.strides[0];
        report.topology = static_cast<uint32_t>(self.geometry.topology);
        std::memcpy(report.vertex_constants, self.geometry.vertex_constants,
                    sizeof(report.vertex_constants));
        std::memcpy(report.pixel_constants, self.pixel_constants, sizeof(report.pixel_constants));
        extra_stage_constants(context, report);
        research(self.research_user.load(std::memory_order_relaxed), &report);
    }
}

rsf_resource_role role_of(const Tap& self, const D3D11_TEXTURE2D_DESC& description,
                          const rsf_frame_shape& shape)
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

    const rsf_role_table table = {sizeof(rsf_role_table), self.role_rules, self.role_count};
    rsf_role_verdict verdict{};
    verdict.struct_size = sizeof(verdict);
    if (!rsf_classify_texture(&table, &facts, &shape, &verdict)) {
        return RSF_ROLE_UNKNOWN;
    }
    return verdict.role;
}

// Whether anything reads the shader resource shadow right now. A tap with no pass callback, no
// watch reports, no candidate prefilter, no divert, no constant override and no research capture
// never looks at it, and then maintaining it is a resource query per binding for nothing.
void refresh_shadow_need(Tap& self)
{
    const rsf_frame_tap_options& options = self.options;
    const bool needed =
        options.on_pass || options.on_target_draw || options.on_input_draw ||
        options.on_candidate_draw || self.research_draw.load(std::memory_order_relaxed) ||
        self.research_before.load(std::memory_order_relaxed) ||
        self.divert_armed.load(std::memory_order_relaxed) != 0 ||
        self.constant_override.load(std::memory_order_relaxed);
    self.shadow_needed.store(needed, std::memory_order_relaxed);
}

// Put one view into the shadow, replacing what the slot held. `resolved` is a reference to the
// texture behind `view` that the caller already took, adopted here, or null to look it up.
void shadow_view(Tap& self, UINT index, ID3D11ShaderResourceView* view, ID3D11Texture2D* resolved)
{
    drop_slot(self, index);
    if (!view) {
        // Unbound, and the slot is now empty, which is what keeps the shadow honest.
        if (resolved) {
            resolved->Release();
        }
        return;
    }
    Tap::Slot& slot = self.slots[index];
    slot.view = view;
    set_occupied(self, index, true);
    slot.texture = resolved ? resolved : texture_behind(view);
    if (!slot.texture) {
        return;  // a buffer or a 3D texture, neither of which is in this set
    }
    if (slot.texture == self.target_texture) {
        // PSSetShaderResources also refuses a read conflicting with the current output.
        drop_slot(self, index);
        return;
    }
    slot.texture->GetDesc(&slot.description);
}

// Bring the shadow in line with whether anything reads it. A shadow that was not being kept is
// seeded from the context, which is exact: the runtime has already unbound what it refused. The one
// inexactness is a plan that substitutes views, whose stand-ins the seed sees until the game binds
// again. One that is no longer read lets go of its references. Render thread.
void sync_shadow(Tap& self, ID3D11DeviceContext* context)
{
    const bool wanted = self.shadow_needed.load(std::memory_order_relaxed);
    if (wanted == self.shadow_live) {
        return;
    }
    self.shadow_live = wanted;
    self.shadow_dirty = true;
    self.input_watch_dirty = true;
    if (!wanted) {
        each_occupied(self, [&](UINT index) { drop_slot(self, index); });
        self.signature_complete = false;
        return;
    }
    ID3D11ShaderResourceView* views[max_examined_views]{};
    context->PSGetShaderResources(0, max_examined_views, views);
    for (UINT index = 0; index < max_examined_views; ++index) {
        if (views[index]) {
            shadow_view(self, index, views[index], nullptr);
            views[index]->Release();
        }
    }
}

void STDMETHODCALLTYPE hooked_ps_set_shader_resources(ID3D11DeviceContext* context, UINT start_slot,
                                                      UINT count,
                                                      ID3D11ShaderResourceView* const* views)
{
    Tap& self = enter_hook();
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
    //
    // A view already in its slot has its texture there. Any other is resolved once, here, and the
    // reference is kept for the shadow below to adopt rather than resolving it again.
    ID3D11ShaderResourceView* substituted[max_examined_views];
    ID3D11Texture2D* resolved[max_examined_views];
    ID3D11ShaderResourceView* const* forwarded = views;
    bool resolving = false;
    if (!inside_hook && context == self.observed_context && views && count > 0 &&
        count <= max_examined_views &&
        self.plan_active.load(std::memory_order_relaxed)) {
        const ReentryGuard substitution_guard;
        resolving = true;
        bool any = false;
        for (UINT index = 0; index < count; ++index) {
            substituted[index] = views[index];
            resolved[index] = nullptr;
            if (!views[index]) {
                continue;
            }
            const UINT slot_index = start_slot + index;
            const Tap::Slot* known = slot_index < max_examined_views ? &self.slots[slot_index] : nullptr;
            ID3D11Texture2D* texture = nullptr;
            if (known && known->view == views[index] && known->texture) {
                texture = known->texture;
            } else {
                texture = resolved[index] = texture_behind(views[index]);
            }
            if (!texture) {
                continue;
            }
            const rsf_frame_tap_substitution* entry =
                applicable_entry(self, texture, &rsf_frame_tap_substitution::shader_view);
            if (!entry) {
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

    if (inside_hook || context != self.observed_context) {
        return;
    }
    const ReentryGuard guard;
    self.calls_seen.fetch_add(1, std::memory_order_relaxed);
    sync_shadow(self, context);

    // Update the shadow of what is bound. Only a slot whose view actually changed costs anything:
    // a pointer compare rejects the rebinding of the same texture, which is most of what a frame
    // does, and only a genuine change pays for the resource query and the classification.
    bool changed = false;
    for (UINT index = 0; index < count && self.shadow_live; ++index) {
        const UINT slot_index = start_slot + index;
        if (slot_index >= max_examined_views) {
            break;
        }
        ID3D11ShaderResourceView* view = views ? views[index] : nullptr;
        if (self.slots[slot_index].view == view) {
            continue;
        }
        changed = true;
        shadow_view(self, slot_index, view, resolving ? resolved[index] : nullptr);
        if (resolving) {
            resolved[index] = nullptr;
        }
    }
    if (resolving) {
        for (UINT index = 0; index < count; ++index) {
            if (resolved[index]) {
                resolved[index]->Release();
            }
        }
    }
    if (!changed) {
        return;
    }
    self.calls_inspected.fetch_add(1, std::memory_order_relaxed);
    self.shadow_dirty = true;
    self.input_watch_dirty = true;
}

// Decide what to bind at render target slot 0, opening any gate this binding opens.
//
// Returns the view to forward, which is the game's own unless the plan replaced it. `substituted`
// is the caller's array to build a replaced set in; only slot 0 is ever replaced, because Unreal
// binds several targets in the GBuffer pass and exactly one everywhere in the frame's tail, and a
// multiple target pass whose first target moved to another resolution is a pass that will not draw
// at all.
// `depth_out` starts as what the game asked for and may be cleared, which is how a promoted target
// avoids being paired with a depth of the wrong size. See `depth_policy`.
// `resolved_out` receives the texture behind slot 0 with a reference the caller owns, when this had
// to look it up, so the shadow need not.
ID3D11RenderTargetView* const* plan_render_targets(Tap& self, ID3D11DeviceContext* context,
                                                   UINT count, ID3D11RenderTargetView* const* views,
                                                   ID3D11RenderTargetView** substituted,
                                                   ID3D11DepthStencilView** depth_out,
                                                   ID3D11Texture2D** resolved_out)
{
    if (inside_hook || context != self.observed_context || !views || count != 1 ||
        !self.plan_active.load(std::memory_order_relaxed)) {
        return views;
    }
    const ReentryGuard substitution_guard;
    if (!views[0]) {
        return views;
    }
    ID3D11Texture2D* texture = texture_behind(views[0]);
    if (!texture) {
        return views;
    }
    *resolved_out = texture;
    // Before the substitution and before the binding is forwarded. A gate is the point in the frame
    // this binding marks, not a consequence of what gets bound there.
    open_gates_for(self, context, texture);

    const rsf_frame_tap_substitution* entry =
        applicable_entry(self, texture, &rsf_frame_tap_substitution::render_view);
    if (!entry) {
        return views;
    }

    // The promoted target is at output resolution and whatever depth the game has bound is still at
    // render resolution. D3D11 rejects that pair, so the pass would draw nothing at all: the flat
    // interface draws bind no depth and survive, and everything depth tested is silently lost.
    if (depth_out && *depth_out) {
        uint32_t depth_width = 0, depth_height = 0, depth_format = 0;
        uint32_t target_width = 0, target_height = 0, target_format = 0;
        const bool measured =
            view_extent(*depth_out, depth_width, depth_height, depth_format) &&
            view_extent(static_cast<ID3D11RenderTargetView*>(entry->render_view), target_width,
                        target_height, target_format);
        if (measured && (depth_width != target_width || depth_height != target_height)) {
            self.depth_mismatches.fetch_add(1, std::memory_order_relaxed);
            self.depth_mismatch_target_width.store(target_width, std::memory_order_relaxed);
            self.depth_mismatch_target_height.store(target_height, std::memory_order_relaxed);
            self.depth_mismatch_depth_width.store(depth_width, std::memory_order_relaxed);
            self.depth_mismatch_depth_height.store(depth_height, std::memory_order_relaxed);
            self.depth_mismatch_depth_format.store(depth_format, std::memory_order_relaxed);
            if (self.plan.depth_policy == RSF_FRAME_TAP_DEPTH_REFUSE) {
                return views;
            }
            if (self.plan.depth_policy != RSF_FRAME_TAP_DEPTH_KEEP) {
                *depth_out = nullptr;
            }
        }
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
    if (self.plan_active.load(std::memory_order_relaxed) && self.target_texture &&
        self.target_count == 1) {
        substituted = applicable_entry(self, self.target_texture,
                                       &rsf_frame_tap_substitution::render_view) != nullptr;
    }
    if (substituted == self.target_substituted) {
        return;
    }
    self.target_substituted = substituted;
    apply_viewport_policy(self, context);
}

// What both binding hooks do once the game's call has been forwarded. The shadow records what is
// actually bound now, not what the game asked for, because the geometry replay reads it back and a
// depth that was dropped is not there to replay against.
void after_set_targets(Tap& self, ID3D11DeviceContext* context, UINT count,
                       ID3D11RenderTargetView* const* views, ID3D11DepthStencilView* depth,
                       ID3D11Texture2D* resolved)
{
    const ReentryGuard guard;
    self.depth_bound = depth != nullptr;
    self.geometry_depth = depth;
    self.target_count = count;
    shadow_render_target(self, (views && count > 0) ? views[0] : nullptr, resolved);
    unbind_target_reads(self);
    settle_target_substitution(self, context);
}

void STDMETHODCALLTYPE hooked_om_set_render_targets(ID3D11DeviceContext* context, UINT count,
                                                    ID3D11RenderTargetView* const* views,
                                                    ID3D11DepthStencilView* depth)
{
    Tap& self = enter_hook();
    const om_set_render_targets_fn forward = self.original_set_targets;
    if (!forward) {
        return;
    }
    ID3D11RenderTargetView* substituted[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT];
    ID3D11DepthStencilView* forwarded_depth = depth;
    ID3D11Texture2D* resolved = nullptr;
    ID3D11RenderTargetView* const* forwarded_targets =
        plan_render_targets(self, context, count, views, substituted, &forwarded_depth, &resolved);
    forward(context, count, forwarded_targets, forwarded_depth);
    if (inside_hook || context != self.observed_context) {
        return;
    }
    after_set_targets(self, context, count, views, forwarded_depth, resolved);
}

// The same binding by another entry point. Unreal's D3D11 backend uses it whenever a pass declares
// an unordered access view, and a shadow that only watched the first call would go stale for every
// pass that does, which in this frame includes the compute-adjacent post process work.
void STDMETHODCALLTYPE hooked_om_set_render_targets_and_uavs(
    ID3D11DeviceContext* context, UINT count, ID3D11RenderTargetView* const* views,
    ID3D11DepthStencilView* depth, UINT uav_start, UINT uav_count,
    ID3D11UnorderedAccessView* const* uavs, const UINT* initial_counts)
{
    Tap& self = enter_hook();
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
    ID3D11DepthStencilView* forwarded_depth = depth;
    ID3D11Texture2D* resolved = nullptr;
    ID3D11RenderTargetView* const* forwarded =
        keeps_targets ? views
                      : plan_render_targets(self, context, count, views, substituted,
                                            &forwarded_depth, &resolved);
    forward(context, count, forwarded, forwarded_depth, uav_start, uav_count, uavs, initial_counts);
    if (inside_hook || context != self.observed_context || keeps_targets) {
        return;
    }
    after_set_targets(self, context, count, views, forwarded_depth, resolved);
}

void STDMETHODCALLTYPE hooked_rs_set_viewports(ID3D11DeviceContext* context, UINT count,
                                               const D3D11_VIEWPORT* viewports)
{
    Tap& self = enter_hook();
    const rs_set_viewports_fn forward = self.original_set_viewports;
    if (!forward) {
        return;
    }
    if (inside_hook || context != self.observed_context ||
        !self.plan_active.load(std::memory_order_relaxed)) {
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
    Tap& self = enter_hook();
    const rs_set_scissor_rects_fn forward = self.original_set_scissors;
    if (!forward) {
        return;
    }
    if (inside_hook || context != self.observed_context ||
        !self.plan_active.load(std::memory_order_relaxed)) {
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
    Tap& self = enter_hook();
    const clear_render_target_view_fn forward = self.original_clear_target;
    if (!forward) {
        return;
    }
    ID3D11RenderTargetView* target = view;
    if (!inside_hook && context == self.observed_context && view &&
        self.plan_active.load(std::memory_order_relaxed)) {
        const ReentryGuard guard;
        // The bound target is nearly always what is cleared, and the shadow has its texture.
        ID3D11Texture2D* owned = nullptr;
        ID3D11Texture2D* texture = view == self.target_view ? self.target_texture : nullptr;
        if (!texture) {
            texture = owned = texture_behind(view);
        }
        if (texture) {
            const rsf_frame_tap_substitution* entry =
                applicable_entry(self, texture, &rsf_frame_tap_substitution::render_view);
            if (entry) {
                target = static_cast<ID3D11RenderTargetView*>(entry->render_view);
            }
        }
        if (owned) {
            owned->Release();
        }
    }
    forward(context, target, colour);
    refresh_hooks(self);
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
    if (!self.options.on_pass) return; // Native owners use graph inputs; keep shadows for captures.
    if (!self.shadow_dirty) {
        return;
    }
    self.shadow_dirty = false;
    if (self.role_count == 0) {
        return;  // no rules, so nothing here can be recognised
    }

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

    each_occupied(self, [&](UINT index) {
        Tap::Slot& entry = self.slots[index];
        if (!entry.texture) {
            return;
        }
        entry.role = role_of(self, entry.description, shape);
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
        } else if (role == RSF_ROLE_HISTORY && !history) {
            // The second target with scene colour's shape. Which of the two holds the accumulated
            // history is not decidable from a descriptor, so this is the remaining candidate and
            // not a demonstrated history buffer.
            history = entry.texture;
        }
    });

    // Scene colour is chosen by its role and by matching the motion target's size, in a second
    // pass because the render resolution is not known until the motion target has been found.
    //
    // It used to be taken as slot 0, on Unreal's post process input convention. The game says
    // otherwise: in the set it actually binds, slot 0 holds R10G10B10A2, which is the GBuffer's
    // normals, and the colour is a floating point target further along. Taking slot 0 would have
    // handed a backend the normal buffer, and produced an image that was wrong rather than absent.
    // Which formats count is the caller's role table; this only insists that the candidate is the
    // size of the motion target.
    if (motion) {
        each_occupied(self, [&](UINT index) {
            const Tap::Slot& entry = self.slots[index];
            if (scene_color || !entry.texture || entry.role != RSF_ROLE_SCENE_COLOR ||
                entry.description.Width != motion_width ||
                entry.description.Height != motion_height) {
                return;
            }
            scene_color = entry.texture;
            scene_color_format = uint32_t(entry.description.Format);
        });
    }

    // Velocity, a 1x1 target and depth bound at the same time is the signature. The format rules
    // for each are the role table's alone, so that this module and the classifier cannot drift apart.
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
        rsf::say(self.options.log, self.options.log_user,
                 "near miss %u: motion %s, depth %s, colour %s, exposure %s. bound set follows",
                 self.described, motion ? "yes" : "no", depth ? "yes" : "no",
                 scene_color ? "yes" : "no", exposure ? "yes" : "no");
        each_occupied(self, [&](UINT index) {
            const Tap::Slot& entry = self.slots[index];
            if (!entry.texture) {
                return;
            }
            rsf::say(self.options.log, self.options.log_user,
                     "  slot %u: %ux%u format %u binds 0x%x mips %u samples %u role %u", index,
                     entry.description.Width, entry.description.Height,
                     unsigned(entry.description.Format), unsigned(entry.description.BindFlags),
                     entry.description.MipLevels, entry.description.SampleDesc.Count,
                     unsigned(entry.role));
        });
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
            rsf::say(self.options.log, self.options.log_user,
                     "reconstruction input set %u at %ux%u, %s history, %s view constants",
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

// Seed inherited state on the owning render thread, also after ClearState/ExecuteCommandList.
// Getters release their references immediately: the context owns the live bindings.
void seed_geometry(Tap& self, ID3D11DeviceContext* context)
{
    auto& g = self.geometry;
    auto drop = [](auto* item) {
        if (item) {
            item->Release();
        }
    };
    ID3D11Buffer* buffers[32]{};
    context->IAGetVertexBuffers(0, 32, buffers, g.strides, g.offsets);
    for (UINT i = 0; i < 32; ++i) {
        g.vertex_buffers[i] = buffers[i];
        drop(buffers[i]);
    }
    ID3D11Buffer* index = nullptr;
    DXGI_FORMAT format{};
    context->IAGetIndexBuffer(&index, &format, &g.index_offset);
    g.index_buffer = index;
    g.index_format = format;
    drop(index);
    ID3D11InputLayout* layout = nullptr;
    context->IAGetInputLayout(&layout);
    g.input_layout = layout;
    drop(layout);
    D3D11_PRIMITIVE_TOPOLOGY topology{};
    context->IAGetPrimitiveTopology(&topology);
    g.topology = topology;
    ID3D11VertexShader* shader = nullptr;
    context->VSGetShader(&shader, nullptr, nullptr);
    g.vertex_shader = shader;
    drop(shader);
    context->VSGetConstantBuffers(0, 14, buffers);
    for (UINT i = 0; i < 14; ++i) {
        g.vertex_constants[i] = buffers[i];
        drop(buffers[i]);
    }
    context->PSGetConstantBuffers(0, 14, buffers);
    for (UINT i = 0; i < 14; ++i) {
        self.pixel_constants[i] = buffers[i];
        drop(buffers[i]);
    }
    self.geometry_valid = true;
}

void report_geometry(Tap& self, ID3D11DeviceContext* context, UINT kind, UINT count, UINT start,
                     INT base, UINT instances = 1, UINT first_instance = 0)
{
    if (!self.options.on_geometry || !self.geometry_depth || self.target_count != 1 ||
        !self.target_texture || self.target_substituted) {
        return;
    }
    if (!self.geometry_valid) {
        seed_geometry(self, context);
    }
    auto& g = self.geometry;
    g.context = context;
    g.target = self.target_texture;
    g.depth_view = self.geometry_depth;
    g.width = self.target_description.Width;
    g.height = self.target_description.Height;
    g.format = self.target_description.Format;
    g.samples = self.target_description.SampleDesc.Count;
    g.kind = kind;
    g.count = count;
    g.start = start;
    g.base_vertex = base;
    g.instances = instances;
    g.start_instance = first_instance;
    self.options.on_geometry(self.options.on_geometry_user, &g);
}

void STDMETHODCALLTYPE hooked_vertex_buffers(ID3D11DeviceContext* c, UINT start, UINT count,
                                             ID3D11Buffer* const* b, const UINT* strides,
                                             const UINT* offsets)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, ID3D11Buffer* const*,
                                        const UINT*, const UINT*);
    reinterpret_cast<Fn>(s.extra_originals[0])(c, start, count, b, strides, offsets);
    if (inside_hook || c != s.observed_context) {
        return;
    }
    for (UINT i = 0; i < count && start + i < 32; ++i) {
        s.geometry.vertex_buffers[start + i] = b ? b[i] : nullptr;
        s.geometry.strides[start + i] = strides ? strides[i] : 0;
        s.geometry.offsets[start + i] = offsets ? offsets[i] : 0;
    }
}
void STDMETHODCALLTYPE hooked_index_buffer(ID3D11DeviceContext* c, ID3D11Buffer* b, DXGI_FORMAT f,
                                           UINT o)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Buffer*, DXGI_FORMAT, UINT);
    reinterpret_cast<Fn>(s.extra_originals[1])(c, b, f, o);
    if (inside_hook || c != s.observed_context) {
        return;
    }
    s.geometry.index_buffer = b;
    s.geometry.index_format = f;
    s.geometry.index_offset = o;
}
void STDMETHODCALLTYPE hooked_layout(ID3D11DeviceContext* c, ID3D11InputLayout* l)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11InputLayout*);
    reinterpret_cast<Fn>(s.extra_originals[2])(c, l);
    if (!inside_hook && c == s.observed_context) {
        s.geometry.input_layout = l;
    }
}
// The pixel shader, the blend and the depth stencil state. Recorded and nothing more: what they
// mean is a game question, and the answer is a pointer comparison against what was named at
// creation. Deliberately not fetched at the draw, where an OMGetBlendState would cost a reference
// count per draw in the frame for a value that changes a few dozen times.
void STDMETHODCALLTYPE hooked_pixel_shader(ID3D11DeviceContext* c, ID3D11PixelShader* p,
                                           ID3D11ClassInstance* const* classes, UINT count)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11PixelShader*,
                                        ID3D11ClassInstance* const*, UINT);
    reinterpret_cast<Fn>(s.extra_originals[13])(c, p, classes, count);
    if (!inside_hook && c == s.observed_context) {
        s.pixel_shader = p;
    }
}
void STDMETHODCALLTYPE hooked_blend_state(ID3D11DeviceContext* c, ID3D11BlendState* b,
                                          const FLOAT factor[4], UINT mask)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11BlendState*, const FLOAT[4],
                                        UINT);
    reinterpret_cast<Fn>(s.extra_originals[14])(c, b, factor, mask);
    if (!inside_hook && c == s.observed_context) {
        s.blend_state = b;
    }
}
void STDMETHODCALLTYPE hooked_depth_stencil_state(ID3D11DeviceContext* c,
                                                  ID3D11DepthStencilState* d, UINT reference)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11DepthStencilState*, UINT);
    reinterpret_cast<Fn>(s.extra_originals[15])(c, d, reference);
    if (!inside_hook && c == s.observed_context) {
        s.depth_stencil_state = d;
    }
}
void STDMETHODCALLTYPE hooked_topology(ID3D11DeviceContext* c, D3D11_PRIMITIVE_TOPOLOGY t)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, D3D11_PRIMITIVE_TOPOLOGY);
    reinterpret_cast<Fn>(s.extra_originals[3])(c, t);
    if (!inside_hook && c == s.observed_context) {
        s.geometry.topology = t;
    }
}
void STDMETHODCALLTYPE hooked_vertex_shader(ID3D11DeviceContext* c, ID3D11VertexShader* v,
                                            ID3D11ClassInstance* const* classes, UINT count)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11VertexShader*,
                                        ID3D11ClassInstance* const*, UINT);
    reinterpret_cast<Fn>(s.extra_originals[4])(c, v, classes, count);
    if (!inside_hook && c == s.observed_context) {
        s.geometry.vertex_shader = v;
    }
}
void STDMETHODCALLTYPE hooked_vertex_constants(ID3D11DeviceContext* c, UINT start, UINT count,
                                               ID3D11Buffer* const* b)
{
    auto& s = enter_hook();
    reinterpret_cast<ps_set_constant_buffers_fn>(s.extra_originals[5])(c, start, count, b);
    if (inside_hook || c != s.observed_context) {
        return;
    }
    for (UINT i = 0; i < count && start + i < 14; ++i) {
        s.geometry.vertex_constants[start + i] = b ? b[i] : nullptr;
    }
}
// One draw the tap looks at, from the divert decision to the reports. `forward` makes the game's call;
// it is invoked after anything that may have moved the runtime's table, so it reads the original
// afresh. The guard is taken before the divert rather than after the draw, because retargeting
// issues context calls of its own and they are not the game's. `before_phase` also offers the draw
// to the research callback that wants to see it ahead of the game's call.
template <class Forward>
void around_draw(Tap& s, ID3D11DeviceContext* c, bool indexed, UINT elements, bool before_phase,
                 UINT geometry_kind, UINT start, INT base, UINT instances, UINT first_instance,
                 Forward&& forward)
{
    if (inside_hook || c != s.observed_context) {
        forward();
        refresh_hooks(s);
        return;
    }
    const ReentryGuard guard;
    sync_shadow(s, c);
    const bool moved = try_divert(s, c, indexed, elements);
    if (!moved) {
        begin_constant_override(s, c, indexed, elements);
    }
    if (before_phase) {
        consider_target_draw(s, c, indexed, elements, true);
        // A before-capture Map can switch the runtime's submission vtable.
        refresh_hooks(s);
    }
    forward();
    refresh_hooks(s);
    if (moved) {
        end_divert(s, c);
    }
    end_constant_override(s, c);
    report_geometry(s, c, geometry_kind, elements, start, base, instances, first_instance);
    consider_bound_set(s, c);
    consider_target_draw(s, c, indexed, elements);
}

void STDMETHODCALLTYPE hooked_indexed_instanced(ID3D11DeviceContext* c, UINT n, UINT instances,
                                                UINT start, INT base, UINT first)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, INT, UINT);
    around_draw(s, c, true, n, false, 3, start, base, instances, first, [&] {
        reinterpret_cast<Fn>(s.extra_originals[6])(c, n, instances, start, base, first);
    });
}
void STDMETHODCALLTYPE hooked_instanced(ID3D11DeviceContext* c, UINT n, UINT instances, UINT start,
                                        UINT first)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, UINT);
    around_draw(s, c, false, n, false, 2, start, 0, instances, first, [&] {
        reinterpret_cast<Fn>(s.extra_originals[7])(c, n, instances, start, first);
    });
}
// Command lists/ClearState invalidate all observation until the engine binds targets again.
void invalidate_geometry(Tap& s)
{
    s.geometry_valid = false;
    s.geometry = rsf_frame_tap_geometry{};
    for (auto& buffer : s.pixel_constants) {
        buffer = nullptr;
    }
    s.geometry_depth = nullptr;
    s.depth_bound = false;
    s.target_count = 0;
    s.target_substituted = false;
    s.game_viewport_count = 0;
    s.game_scissor_count = 0;
    ID3D11Buffer* constants = nullptr;
    {
        std::lock_guard<std::mutex> lock(s.guard);
        constants = s.view_constants;
        s.view_constants = nullptr;
        s.view_constants_hint.store(nullptr, std::memory_order_relaxed);
    }
    if (constants) {
        constants->Release();
    }
    each_occupied(s, [&](UINT index) { drop_slot(s, index); });
    s.shadow_dirty = true;
    s.signature_complete = false;
    s.input_watch_dirty = true;
    shadow_render_target(s, nullptr);
}
void STDMETHODCALLTYPE hooked_clear_state(ID3D11DeviceContext* c)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*);
    reinterpret_cast<Fn>(s.extra_originals[8])(c);
    if (!inside_hook && c == s.observed_context) {
        invalidate_geometry(s);
    }
}
void STDMETHODCALLTYPE hooked_execute(ID3D11DeviceContext* c, ID3D11CommandList* list, BOOL restore)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11CommandList*, BOOL);
    reinterpret_cast<Fn>(s.extra_originals[9])(c, list, restore);
    if (!inside_hook && c == s.observed_context && !restore) {
        invalidate_geometry(s);
    }
}
// The draws whose arguments live in a buffer or come from the stream-out count: nothing to look at
// but the fact that one happened.
template <size_t Original, class... Arguments>
void STDMETHODCALLTYPE indirect_draw(ID3D11DeviceContext* c, Arguments... arguments)
{
    auto& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, Arguments...);
    reinterpret_cast<Fn>(s.extra_originals[Original])(c, arguments...);
    refresh_hooks(s);
    if (inside_hook || c != s.observed_context) {
        return;
    }
    const ReentryGuard guard;
    report_geometry(s, c, 4, 0, 0, 0);
}

struct ExtraHook {
    size_t slot;
    void* replacement;
};
// The hooks that observe state or draws, indexed as `extra_originals` is: the hooks above read their
// original by that number.
const ExtraHook extra_hooks[] = {
    {18, reinterpret_cast<void*>(&hooked_vertex_buffers)},
    {19, reinterpret_cast<void*>(&hooked_index_buffer)},
    {17, reinterpret_cast<void*>(&hooked_layout)},
    {24, reinterpret_cast<void*>(&hooked_topology)},
    {11, reinterpret_cast<void*>(&hooked_vertex_shader)},
    {7, reinterpret_cast<void*>(&hooked_vertex_constants)},
    {20, reinterpret_cast<void*>(&hooked_indexed_instanced)},
    {21, reinterpret_cast<void*>(&hooked_instanced)},
    {110, reinterpret_cast<void*>(&hooked_clear_state)},
    {58, reinterpret_cast<void*>(&hooked_execute)},
    {39, reinterpret_cast<void*>(&indirect_draw<10, ID3D11Buffer*, UINT>)},
    {40, reinterpret_cast<void*>(&indirect_draw<11, ID3D11Buffer*, UINT>)},
    {38, reinterpret_cast<void*>(&indirect_draw<12>)},
    {9, reinterpret_cast<void*>(&hooked_pixel_shader)},
    {35, reinterpret_cast<void*>(&hooked_blend_state)},
    {36, reinterpret_cast<void*>(&hooked_depth_stencil_state)},
};
static_assert(sizeof(extra_hooks) / sizeof(extra_hooks[0]) == sizeof(Tap::extra_originals) / sizeof(void*),
              "one original per hook");

// Pass-through hooks for the flush-class calls and for the rest of the work-submission family.
//
// None of these is observed. Each exists so the vtable rewrite a call of its kind causes is noticed
// on the way out, by `refresh_hooks`, rather than by whatever hooked call the game happens to make
// next, which for a draw right after a clear may be nothing. Indices are positions in
// `pass_originals`, in the order `pass_hooks` lists them at install.
template <size_t Original, class... Arguments>
void STDMETHODCALLTYPE pass_through(ID3D11DeviceContext* c, Arguments... arguments)
{
    Tap& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, Arguments...);
    reinterpret_cast<Fn>(s.pass_originals[Original])(c, arguments...);
    refresh_hooks(s);
}

// Whether `resource` is a constant buffer the watch wants, and if so its width; zero when not.
//
// Only a "no" is remembered per address, so a texture upload pays two virtual calls once and not on
// every call. An address can be reused by another resource, so the table is emptied when the epoch
// moves (each frame, and when the watch changes) and after a fixed number of lookups. A "yes" is
// always asked again: its width becomes a memory size the caller copies and hands to the watch, and
// a texture or a differently sized buffer at a recycled address must not be taken for it. Render
// thread only.
uint32_t watched_buffer(Tap& s, ID3D11Resource* resource)
{
    const uint32_t epoch = s.verdict_epoch.load(std::memory_order_relaxed);
    if (epoch != s.verdict_epoch_seen || ++s.verdict_lookups >= Tap::verdict_lifetime) {
        s.verdict_epoch_seen = epoch;
        s.verdict_lookups = 0;
        for (Tap::BufferVerdict& entry : s.verdicts) {
            entry = Tap::BufferVerdict{};
        }
    }
    Tap::BufferVerdict& cached =
        s.verdicts[(reinterpret_cast<uintptr_t>(resource) >> 4) % Tap::verdict_slots];
    if (cached.resource == resource && cached.bytes == 0) {
        return 0;
    }
    uint32_t bytes = 0;
    D3D11_RESOURCE_DIMENSION dimension = D3D11_RESOURCE_DIMENSION_UNKNOWN;
    resource->GetType(&dimension);
    if (dimension == D3D11_RESOURCE_DIMENSION_BUFFER) {
        D3D11_BUFFER_DESC description{};
        static_cast<ID3D11Buffer*>(resource)->GetDesc(&description);
        const uint32_t watched = s.constant_watch_bytes.load(std::memory_order_relaxed);
        const bool wanted =
            watched == 0 ? description.ByteWidth <= 4096u : description.ByteWidth == watched;
        if (wanted && (description.BindFlags & D3D11_BIND_CONSTANT_BUFFER) != 0) {
            bytes = description.ByteWidth;
        }
    }
    cached = Tap::BufferVerdict{resource, bytes};
    return bytes;
}

HRESULT STDMETHODCALLTYPE hooked_map(ID3D11DeviceContext* c, ID3D11Resource* resource,
                                     UINT subresource, D3D11_MAP kind, UINT flags,
                                     D3D11_MAPPED_SUBRESOURCE* mapped)
{
    Tap& s = enter_hook();
    using Fn = HRESULT(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT, D3D11_MAP,
                                           UINT, D3D11_MAPPED_SUBRESOURCE*);
    const HRESULT result =
        reinterpret_cast<Fn>(s.pass_originals[0])(c, resource, subresource, kind, flags, mapped);
    refresh_hooks(s);
    // A constant buffer being filled: remember where, so its contents can be read at Unmap.
    if (s.constant_watch.load(std::memory_order_acquire) && SUCCEEDED(result) && mapped &&
        mapped->pData && resource && kind == D3D11_MAP_WRITE_DISCARD && subresource == 0 &&
        !inside_hook && c == s.observed_context) {
        const uint32_t bytes = watched_buffer(s, resource);
        if (bytes != 0) {
            for (Tap::PendingMap& pending : s.pending_maps) {
                if (!pending.resource || pending.resource == resource) {
                    pending.resource = resource;
                    pending.data = mapped->pData;
                    pending.bytes = bytes;
                    break;
                }
            }
        }
    }
    return result;
}
void STDMETHODCALLTYPE hooked_unmap(ID3D11DeviceContext* c, ID3D11Resource* resource,
                                    UINT subresource)
{
    Tap& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT);
    // Before forwarding, while the mapped memory is still the game's to read.
    if (resource && c == s.observed_context) {
        for (Tap::PendingMap& pending : s.pending_maps) {
            if (pending.resource != resource) {
                continue;
            }
            const rsf_frame_tap_constants_fn watch =
                s.constant_watch.load(std::memory_order_acquire);
            if (watch && !inside_hook) {
                const ReentryGuard guard;
                watch(s.constant_watch_user.load(std::memory_order_relaxed), resource,
                      pending.data, pending.bytes);
            }
            pending = Tap::PendingMap{};
            break;
        }
    }
    reinterpret_cast<Fn>(s.pass_originals[1])(c, resource, subresource);
    refresh_hooks(s);
}
void STDMETHODCALLTYPE hooked_dispatch(ID3D11DeviceContext* c, UINT x, UINT y, UINT z)
{
    Tap& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT);
    reinterpret_cast<Fn>(s.pass_originals[3])(c, x, y, z);
    refresh_hooks(s);
    const auto fn = s.research_compute.load(std::memory_order_acquire);
    if (fn && !inside_hook && c == s.observed_context) {
        const ReentryGuard guard;
        fn(s.research_user.load(std::memory_order_relaxed), c, x, y, z, nullptr, 0);
    }
}
void STDMETHODCALLTYPE hooked_dispatch_indirect(ID3D11DeviceContext* c, ID3D11Buffer* arguments,
                                                UINT offset)
{
    Tap& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Buffer*, UINT);
    reinterpret_cast<Fn>(s.pass_originals[4])(c, arguments, offset);
    refresh_hooks(s);
    const auto fn = s.research_compute.load(std::memory_order_acquire);
    if (fn && !inside_hook && c == s.observed_context) {
        const ReentryGuard guard;
        fn(s.research_user.load(std::memory_order_relaxed), c, 0, 0, 0, arguments, offset);
    }
}
void STDMETHODCALLTYPE hooked_copy_resource(ID3D11DeviceContext* c, ID3D11Resource* destination,
                                            ID3D11Resource* source)
{
    Tap& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, ID3D11Resource*);
    // A copy between textures the plan has promoted goes between their stand-ins, so a result the
    // game copies from one promoted target into another arrives at output resolution. AC7 copies
    // its recombined colour back into scene colour this way before the tonemap reads it.
    ID3D11Resource* forwarded_destination = destination;
    ID3D11Resource* forwarded_source = source;
    if (!inside_hook && c == s.observed_context && s.plan_active.load(std::memory_order_relaxed)) {
        const ReentryGuard guard;
        ID3D11Resource* promoted_destination = promoted_resource(s, destination);
        ID3D11Resource* promoted_source = promoted_resource(s, source);
        if (promoted_destination && promoted_source) {
            forwarded_destination = promoted_destination;
            forwarded_source = promoted_source;
            s.copies_redirected.fetch_add(1, std::memory_order_relaxed);
        } else if (promoted_destination || promoted_source) {
            // Half a pair would copy between different sizes, which D3D11 drops. Counted, so a
            // run can say the case exists.
            s.copies_mismatched.fetch_add(1, std::memory_order_relaxed);
        }
        if (promoted_destination) {
            promoted_destination->Release();
        }
        if (promoted_source) {
            promoted_source->Release();
        }
    }
    reinterpret_cast<Fn>(s.pass_originals[6])(c, forwarded_destination, forwarded_source);
    refresh_hooks(s);
}
// A shader's own constants, on their way to the GPU. Unreal 4.18's D3D11 RHI sends them with
// UpdateSubresource from its CPU shadow (WindowsD3D11ConstantBuffer.cpp:90), whole sub-buffer at a
// time, right before the draw; only pooled uniform buffers go through Map. The watch sees these
// too, on a copy: the game's shadow is the game's, and what it asked to upload is what the next
// upload must start from.
const void* watch_update(Tap& s, ID3D11DeviceContext* c, ID3D11Resource* destination,
                         UINT subresource, const D3D11_BOX* box, const void* data,
                         UINT row_pitch, void* scratch)
{
    const rsf_frame_tap_constants_fn watch = s.constant_watch.load(std::memory_order_acquire);
    if (!watch || inside_hook || c != s.observed_context || !destination || !data || box ||
        subresource != 0 || row_pitch == 0 || row_pitch > 4096u) {
        return data;
    }
    const uint32_t bytes = watched_buffer(s, destination);
    if (bytes == 0 || row_pitch > bytes) {
        return data;
    }
    void* user = s.constant_watch_user.load(std::memory_order_relaxed);
    const ReentryGuard guard;
    s.updates_watched.fetch_add(1, std::memory_order_relaxed);
    if (!s.constant_watch_writable.load(std::memory_order_relaxed)) {
        // A watch that only reads is shown the game's own memory.
        watch(user, destination, const_cast<void*>(data), row_pitch);
        return data;
    }
    std::memcpy(scratch, data, row_pitch);
    watch(user, destination, scratch, row_pitch);
    return scratch;
}

void STDMETHODCALLTYPE hooked_update_subresource(ID3D11DeviceContext* c,
                                                 ID3D11Resource* destination, UINT subresource,
                                                 const D3D11_BOX* box, const void* data,
                                                 UINT row_pitch, UINT depth_pitch)
{
    Tap& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT,
                                        const D3D11_BOX*, const void*, UINT, UINT);
    alignas(16) unsigned char scratch[4096];
    data = watch_update(s, c, destination, subresource, box, data, row_pitch, scratch);
    reinterpret_cast<Fn>(s.pass_originals[7])(c, destination, subresource, box, data, row_pitch,
                                              depth_pitch);
    refresh_hooks(s);
}
void STDMETHODCALLTYPE hooked_update_subresource1(ID3D11DeviceContext* c,
                                                  ID3D11Resource* destination, UINT subresource,
                                                  const D3D11_BOX* box, const void* data,
                                                  UINT row_pitch, UINT depth_pitch, UINT flags)
{
    Tap& s = enter_hook();
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT,
                                        const D3D11_BOX*, const void*, UINT, UINT, UINT);
    alignas(16) unsigned char scratch[4096];
    data = watch_update(s, c, destination, subresource, box, data, row_pitch, scratch);
    reinterpret_cast<Fn>(s.pass_originals[15])(c, destination, subresource, box, data, row_pitch,
                                               depth_pitch, flags);
    refresh_hooks(s);
}

// Slot, hook and index into `pass_originals`, and whether the slot only exists on
// ID3D11DeviceContext1. Slot numbers count the three IUnknown and four ID3D11DeviceChild entries.
struct PassHook {
    size_t slot;
    void* replacement;
    size_t original;
    bool context1;
};
const PassHook pass_hooks[] = {
    {14, reinterpret_cast<void*>(&hooked_map), 0, false},
    {15, reinterpret_cast<void*>(&hooked_unmap), 1, false},
    {111, reinterpret_cast<void*>(&pass_through<2>), 2, false},
    {41, reinterpret_cast<void*>(&hooked_dispatch), 3, false},
    {42, reinterpret_cast<void*>(&hooked_dispatch_indirect), 4, false},
    {46,
     reinterpret_cast<void*>(&pass_through<5, ID3D11Resource*, UINT, UINT, UINT, UINT,
                                           ID3D11Resource*, UINT, const D3D11_BOX*>),
     5, false},
    {47, reinterpret_cast<void*>(&hooked_copy_resource), 6, false},
    {48, reinterpret_cast<void*>(&hooked_update_subresource), 7, false},
    {49,
     reinterpret_cast<void*>(
         &pass_through<8, ID3D11Buffer*, UINT, ID3D11UnorderedAccessView*>),
     8, false},
    {51,
     reinterpret_cast<void*>(&pass_through<9, ID3D11UnorderedAccessView*, const UINT*>), 9,
     false},
    {52,
     reinterpret_cast<void*>(&pass_through<10, ID3D11UnorderedAccessView*, const FLOAT*>), 10,
     false},
    {53,
     reinterpret_cast<void*>(
         &pass_through<11, ID3D11DepthStencilView*, UINT, FLOAT, UINT8>),
     11, false},
    {54, reinterpret_cast<void*>(&pass_through<12, ID3D11ShaderResourceView*>), 12, false},
    {57,
     reinterpret_cast<void*>(&pass_through<13, ID3D11Resource*, UINT, ID3D11Resource*, UINT,
                                           DXGI_FORMAT>),
     13, false},
    // ID3D11DeviceContext1's two members of the family. Patched only where the context has that
    // interface, because a table for the base interface has no slot 115.
    {115,
     reinterpret_cast<void*>(&pass_through<14, ID3D11Resource*, UINT, UINT, UINT, UINT,
                                           ID3D11Resource*, UINT, const D3D11_BOX*, UINT>),
     14, true},
    {116, reinterpret_cast<void*>(&hooked_update_subresource1), 15, true},
};

void STDMETHODCALLTYPE hooked_draw_indexed(ID3D11DeviceContext* context, UINT index_count,
                                           UINT start_index, INT base_vertex)
{
    Tap& self = enter_hook();
    if (!self.original_draw_indexed) {
        return;
    }
    around_draw(self, context, true, index_count, true, 1, start_index, base_vertex, 1, 0, [&] {
        self.original_draw_indexed(context, index_count, start_index, base_vertex);
    });
}

void STDMETHODCALLTYPE hooked_draw(ID3D11DeviceContext* context, UINT vertex_count,
                                   UINT start_vertex)
{
    Tap& self = enter_hook();
    if (!self.original_draw) {
        return;
    }
    around_draw(self, context, false, vertex_count, true, 0, start_vertex, 0, 1, 0,
                [&] { self.original_draw(context, vertex_count, start_vertex); });
}

// The game's object behind a binding, which is the clone's original when the bias put it there.
ID3D11SamplerState* sampler_original(const Tap& self, ID3D11SamplerState* sampler)
{
    const auto found = self.clone_originals.find(sampler);
    return found == self.clone_originals.end() ? sampler : found->second;
}
// What to bind for one of the game's samplers at the current bias. Only filters that blend between
// mips with a mip range are biased, as 4.27 biases only the world texture group's samplers;
// comparison, min/max and point-mip samplers pass through, as does anything at zero bias.
ID3D11SamplerState* biased_sampler(Tap& self, ID3D11DeviceContext* context, ID3D11SamplerState* original)
{
    if (!original || self.sampler_bias == 0) return original;
    auto found = self.biased_samplers.find(original);
    if (found == self.biased_samplers.end()) {
        // Pinned so the address cannot be reused by another sampler while it keys this cache.
        original->AddRef();
        found = self.biased_samplers.emplace(original, Tap::BiasedSampler{}).first;
    }
    Tap::BiasedSampler& entry = found->second;
    if (entry.bias == self.sampler_bias) return entry.clone ? entry.clone : original;
    if (entry.clone) {
        self.clone_originals.erase(entry.clone);
        entry.clone->Release();
        entry.clone = nullptr;
    }
    entry.bias = self.sampler_bias;
    D3D11_SAMPLER_DESC desc{}; original->GetDesc(&desc);
    if (!(desc.Filter & 0x1) || (desc.Filter & 0x180) || !(desc.MaxLOD > desc.MinLOD)) return original;
    desc.MipLODBias = std::clamp(desc.MipLODBias + self.sampler_bias, -16.0f, 15.99f);
    ID3D11Device* device = nullptr; context->GetDevice(&device);
    if (device) { device->CreateSamplerState(&desc, &entry.clone); device->Release(); }
    if (!entry.clone) return original;
    self.clone_originals.emplace(entry.clone, original);
    return entry.clone;
}
void STDMETHODCALLTYPE hooked_ps_set_samplers(ID3D11DeviceContext* context, UINT start_slot, UINT count,
                                              ID3D11SamplerState* const* samplers)
{
    Tap& self = enter_hook();
    const ps_set_samplers_fn forward = self.original_set_samplers;
    if (!forward) return;
    // With no bias there is nothing to swap and nothing to remember: set_sampler_bias reads the
    // bindings back from the context, so this stays a plain forward.
    if (!self.sampler_bias_active.load(std::memory_order_acquire) || inside_hook ||
        context != self.observed_context || !samplers || count == 0 ||
        start_slot >= D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT ||
        count > D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT - start_slot) {
        forward(context, start_slot, count, samplers);
        return;
    }
    ID3D11SamplerState* bound[D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT]{};
    {
        std::lock_guard<std::mutex> lock(self.sampler_guard);
        ID3D11SamplerState* originals[D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT]{};
        for (UINT index = 0; index < count; ++index) {
            originals[index] = sampler_original(self, samplers[index]);
        }
        for (UINT index = 0; index < count; ++index) {
            bound[index] = biased_sampler(self, context, originals[index]);
        }
    }
    forward(context, start_slot, count, bound);
}

// Keep `buffer` as the most recent view constants, with a reference of its own.
void keep_view_constants(Tap& self, ID3D11Buffer* buffer)
{
    buffer->AddRef();
    ID3D11Buffer* previous = nullptr;
    bool kept = false;
    {
        std::lock_guard<std::mutex> lock(self.guard);
        // A call still in flight when uninstall ran must not hand a reference back to a tap
        // that has already dropped everything: nothing would ever release it, and it would
        // outlive the module holding a game buffer alive.
        if (self.installed) {
            previous = self.view_constants;
            self.view_constants = buffer;
            self.view_constants_hint.store(buffer, std::memory_order_relaxed);
            kept = true;
        }
    }
    // Both releases sit outside the lock: a final release runs the object's destructor.
    if (!kept) {
        buffer->Release();
    }
    if (previous) {
        previous->Release();
    }
}

void STDMETHODCALLTYPE hooked_ps_set_constant_buffers(ID3D11DeviceContext* context, UINT start_slot,
                                                      UINT count, ID3D11Buffer* const* buffers)
{
    Tap& self = enter_hook();
    const ps_set_constant_buffers_fn forward = self.original_set_constants;
    if (!forward) {
        return;
    }
    forward(context, start_slot, count, buffers);

    if (inside_hook || context != self.observed_context || count == 0) {
        return;
    }
    // The view buffer is only ever handed to the pass callback, so without one nothing here asks a
    // buffer its size. A slot rebound to what it already held was looked at when it was first bound.
    const uint32_t wanted = self.options.on_pass ? self.options.view_constant_bytes : 0;
    ID3D11Buffer* view = nullptr;
    for (UINT index = 0; index < count; ++index) {
        ID3D11Buffer* buffer = buffers ? buffers[index] : nullptr;
        const UINT slot = start_slot + index;
        if (slot < 14) {
            const bool unchanged = self.pixel_constants[slot] == buffer;
            self.pixel_constants[slot] = buffer;
            if (unchanged) {
                continue;
            }
        }
        if (!wanted || !buffer || view) {
            continue;
        }
        D3D11_BUFFER_DESC description{};
        buffer->GetDesc(&description);
        // Keep the most recent, for the reason the observer keeps the most recent of each size:
        // the contents change every frame, so an older buffer describes a frame nobody asked
        // about. Nothing here says this is the view buffer; picking among the buffers of that size
        // is the caller's job.
        if (description.ByteWidth == wanted) {
            view = buffer;
        }
    }
    if (view && view != self.view_constants_hint.load(std::memory_order_relaxed)) {
        keep_view_constants(self, view);
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
    const rsf_role_table* roles = options->roles;
    if (roles && (roles->struct_size < sizeof(rsf_role_table) ||
                  roles->count > RSF_FRAME_TAP_MAX_ROLE_RULES ||
                  (roles->count != 0 && !roles->rules))) {
        // Refused rather than truncated, for the reason a candidate set is: a table cut short is a
        // role that stops being recognised, which looks like a rule that is wrong.
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }

    Tap& self = tap();
    std::unique_lock<std::mutex> lock(self.guard);
    if (self.installed) {
        return RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED;
    }

    self.options = *options;
    self.options.roles = nullptr;  // the copy below is what is used
    self.role_count = roles ? roles->count : 0;
    if (self.role_count != 0) {
        std::copy_n(roles->rules, self.role_count, self.role_rules);
    }
    self.observed_context = static_cast<ID3D11DeviceContext*>(device_context);
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
    //
    // Gathered into one table the refresh and the uninstall walk as a unit: the observed hooks, then
    // the pass-throughs, of which ID3D11DeviceContext1's two only where the context has that
    // interface.
    self.patch_count = 0;
    auto add = [&](size_t slot, void* replacement, void** original) {
        self.patches[self.patch_count++] = Tap::Patch{slot, replacement, original};
    };
    for (size_t index = 0; index < std::size(extra_hooks); ++index) {
        add(extra_hooks[index].slot, extra_hooks[index].replacement, &self.extra_originals[index]);
    }
    add(slot_ps_set_shader_resources, reinterpret_cast<void*>(&hooked_ps_set_shader_resources),
        reinterpret_cast<void**>(&self.original_set_views));
    add(slot_ps_set_constant_buffers, reinterpret_cast<void*>(&hooked_ps_set_constant_buffers),
        reinterpret_cast<void**>(&self.original_set_constants));
    add(slot_ps_set_samplers, reinterpret_cast<void*>(&hooked_ps_set_samplers),
        reinterpret_cast<void**>(&self.original_set_samplers));
    add(slot_draw_indexed, reinterpret_cast<void*>(&hooked_draw_indexed),
        reinterpret_cast<void**>(&self.original_draw_indexed));
    add(slot_draw, reinterpret_cast<void*>(&hooked_draw),
        reinterpret_cast<void**>(&self.original_draw));
    add(slot_om_set_render_targets, reinterpret_cast<void*>(&hooked_om_set_render_targets),
        reinterpret_cast<void**>(&self.original_set_targets));
    add(slot_om_set_render_targets_and_uavs,
        reinterpret_cast<void*>(&hooked_om_set_render_targets_and_uavs),
        reinterpret_cast<void**>(&self.original_set_targets_and_uavs));
    add(slot_rs_set_viewports, reinterpret_cast<void*>(&hooked_rs_set_viewports),
        reinterpret_cast<void**>(&self.original_set_viewports));
    add(slot_rs_set_scissor_rects, reinterpret_cast<void*>(&hooked_rs_set_scissor_rects),
        reinterpret_cast<void**>(&self.original_set_scissors));
    add(slot_clear_render_target_view, reinterpret_cast<void*>(&hooked_clear_render_target_view),
        reinterpret_cast<void**>(&self.original_clear_target));

    bool context1 = false;
    {
        ID3D11DeviceContext1* extended = nullptr;
        if (SUCCEEDED(self.observed_context->QueryInterface(
                __uuidof(ID3D11DeviceContext1), reinterpret_cast<void**>(&extended))) &&
            extended) {
            context1 = true;
            extended->Release();
        }
    }
    for (const PassHook& pass : pass_hooks) {
        if (pass.context1 && !context1) {
            continue;
        }
        add(pass.slot, pass.replacement, &self.pass_originals[pass.original]);
    }
    self.sentinel_replacement = reinterpret_cast<void*>(&hooked_draw);

    for (size_t applied = 0; applied < self.patch_count; ++applied) {
        Tap::Patch& patch = self.patches[applied];
        if (rsf::patch_slot(self.vtable, patch.slot, patch.replacement, patch.original)) {
            continue;
        }
        while (applied-- > 0) {
            rsf::patch_slot(self.vtable, self.patches[applied].slot, *self.patches[applied].original,
                            nullptr);
            *self.patches[applied].original = nullptr;
        }
        self.patch_count = 0;
        return RSF_FRAME_TAP_ERROR_PATCH_FAILED;
    }

    self.vtable_refreshes.store(0, std::memory_order_relaxed);
    self.shadow_live = false;
    self.verdict_epoch.fetch_add(1, std::memory_order_relaxed);
    self.installed = true;
    refresh_shadow_need(self);
    lock.unlock();
    if (options->on_pass && roles == nullptr) {
        rsf::say(options->log, options->log_user,
                 "frame tap: a pass callback and no role table, so no input set will be recognised");
    }
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_sampler_bias(float bias)
{
    if (!std::isfinite(bias) || bias < -16.0f || bias > 0.0f) return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    Tap& self = tap();
    if (!self.installed || !self.observed_context || !self.original_set_samplers) return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
    std::lock_guard<std::mutex> lock(self.sampler_guard);
    if (self.sampler_bias == bias) return RSF_FRAME_TAP_OK;
    self.sampler_bias = bias;
    // Unreal's state cache skips a sampler it believes is already bound, so the live bindings are
    // rewritten here: with clones as the bias starts, and with the game's own as it ends. They are
    // read back from the context, which holds either kind, and the clones are mapped to their
    // originals.
    ID3D11SamplerState* current[D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT]{};
    self.observed_context->PSGetSamplers(0, D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT, current);
    // Every slot is resolved to its original before any is re-biased: re-biasing drops the old
    // clone's mapping, and a later slot still holding that clone would then be taken for a game
    // sampler and biased a second time.
    ID3D11SamplerState* originals[D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT]{};
    for (UINT index = 0; index < D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT; ++index)
        originals[index] = sampler_original(self, current[index]);
    ID3D11SamplerState* bound[D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT]{};
    for (UINT index = 0; index < D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT; ++index)
        bound[index] = biased_sampler(self, self.observed_context, originals[index]);
    self.original_set_samplers(self.observed_context, 0, D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT, bound);
    for (ID3D11SamplerState* sampler : current)
        if (sampler) sampler->Release();
    self.sampler_bias_active.store(bias != 0, std::memory_order_release);
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_refresh(void)
{
    Tap& self = tap();
    if (!self.installed) {
        return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
    }
    refresh_hooks(self);
    // Once a frame from the present hook, which is also when the lookups of resource addresses stop
    // being trusted.
    self.verdict_epoch.fetch_add(1, std::memory_order_relaxed);
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_uninstall(void)
{
    Tap& self = tap();
    self.research_draw.store(nullptr, std::memory_order_release);
    self.research_before.store(nullptr, std::memory_order_release);
    self.research_compute.store(nullptr, std::memory_order_release);
    {
        std::lock_guard<std::mutex> lock(self.guard);
        if (!self.installed) {
            return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
        }
        // Cleared before the vtable goes back, so a call already inside a hook stops substituting
        // rather than reaching for views the caller is about to release.
        self.plan_active.store(false, std::memory_order_relaxed);
        self.divert_armed.store(0, std::memory_order_relaxed);
        self.constant_override.store(nullptr, std::memory_order_relaxed);
        self.constant_override_format.store(0, std::memory_order_relaxed);
        self.constant_watch_bytes.store(0, std::memory_order_relaxed);
        self.constant_watch.store(nullptr, std::memory_order_relaxed);
        self.constant_watch_writable.store(true, std::memory_order_relaxed);
        self.verdict_epoch.fetch_add(1, std::memory_order_relaxed);
        self.shadow_needed.store(false, std::memory_order_relaxed);
        // Off before the table goes back, so a hook still in flight cannot re-apply what this is
        // removing. Then everything the table names, the pass-throughs included, back to whatever
        // the runtime last had there. On a runtime that rewrites its table that is the variant for
        // its current state: a rewrite since would have removed the hook, and the refresh that put
        // it back recorded the new one.
        self.installed = false;
        for (size_t index = 0; index < self.patch_count; ++index) {
            rsf::patch_slot(self.vtable, self.patches[index].slot, *self.patches[index].original,
                            nullptr);
        }
        self.patch_count = 0;
        // The originals are deliberately kept. A call that entered a hook before the vtable was
        // restored still has to forward, and clearing them turns that race from a stale hook into a
        // null call. They stay valid for as long as the runtime is loaded, and a later install
        // reads them from the vtable again.
        self.layer_target = nullptr;
        self.layer_texture = nullptr;
        self.verdict = nullptr;
    }

    {
        std::lock_guard<std::mutex> lock(self.sampler_guard);
        for (auto& [original, entry] : self.biased_samplers) {
            if (entry.clone) entry.clone->Release();
            original->Release();
        }
        self.biased_samplers.clear();
        self.clone_originals.clear();
        self.sampler_bias = 0;
        self.sampler_bias_active.store(false, std::memory_order_release);
    }

    // The patched blend states are ours: created here, cached here, and released here. Keyed by the
    // game's pointer, so leaving them across an uninstall would leave a cache that could answer for
    // an address the game has since reused.
    for (uint32_t index = 0; index < self.patched_blend_count; ++index) {
        if (self.patched_blends[index].patched) {
            self.patched_blends[index].patched->Release();
        }
        self.patched_blends[index] = Tap::PatchedBlend{};
    }
    self.patched_blend_count = 0;

    // Everything the tap holds of the game's: the view constants, the shadowed bindings with one
    // reference per occupied slot, and the render target. Released after the vtable is restored, so
    // a hook still in flight cannot find a slot emptied underneath it, which narrows the same window
    // uninstall already has rather than opening a new one.
    invalidate_geometry(self);
    self.shadow_dirty = false;
    self.shadow_live = false;
    self.input_watch = nullptr;
    self.input_watch_bound = false;
    self.observed_context = nullptr;

    // The watches are cleared with them: they name textures the caller owns, and leaving them set
    // across an uninstall would have a later install start matching addresses from a session that
    // has ended.
    for (uint32_t index = 0; index < RSF_FRAME_TAP_WATCH_SLOTS; ++index) {
        self.watch[index].store(nullptr, std::memory_order_relaxed);
        self.watch_budget[index].store(0, std::memory_order_relaxed);
    }
    self.plan = rsf_frame_tap_plan{};
    close_gates(self);
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
        close_gates(self);
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
    close_gates(self);
    self.target_substituted = false;
    self.plan_active.store(true, std::memory_order_relaxed);
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_end_frame(void)
{
    Tap& self = tap();
    self.verdict_epoch.fetch_add(1, std::memory_order_relaxed);
    if (!self.plan_active.load(std::memory_order_relaxed)) {
        return RSF_FRAME_TAP_OK;
    }
    // No lock. This is called from the present hook on the render thread, which is the same thread
    // the gates are opened on, and taking the tap's lock on a per frame path would put the render
    // thread behind whichever worker happens to be reading status.
    close_gates(self);
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_watch_input(void* texture)
{
    Tap& self = tap();
    if (!self.installed) {
        return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
    }
    self.input_watch = texture;
    self.input_watch_dirty = true;
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_constant_watch(uint32_t bytes,
                                                                  rsf_frame_tap_constants_fn fn,
                                                                  void* user)
{
    Tap& self = tap();
    self.constant_watch.store(nullptr, std::memory_order_release);
    self.constant_watch_user.store(user, std::memory_order_relaxed);
    self.constant_watch_bytes.store(bytes, std::memory_order_relaxed);
    // What was decided about each buffer was decided for the old width.
    self.verdict_epoch.fetch_add(1, std::memory_order_relaxed);
    // A new watch starts as a writer, whatever the last one said, so the window between arming it
    // and its owner saying it only reads is the safe one: a copy, never the game's memory.
    self.constant_watch_writable.store(true, std::memory_order_relaxed);
    self.constant_watch.store(fn, std::memory_order_release);
    return RSF_FRAME_TAP_OK;
}

extern "C" void rsf_frame_tap_set_constant_watch_writable(uint32_t writable)
{
    tap().constant_watch_writable.store(writable != 0, std::memory_order_relaxed);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_research_callbacks(
    rsf_frame_tap_target_fn draw, rsf_frame_tap_compute_fn compute, void* user)
{
    return rsf_frame_tap_set_research_phase_callbacks(nullptr, draw, compute, user);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_research_phase_callbacks(
    rsf_frame_tap_target_fn before, rsf_frame_tap_target_fn after,
    rsf_frame_tap_compute_fn compute, void* user)
{
    Tap& self = tap();
    if (!self.installed) return RSF_FRAME_TAP_ERROR_NOT_INSTALLED;
    self.research_draw.store(nullptr, std::memory_order_release);
    self.research_before.store(nullptr, std::memory_order_release);
    self.research_compute.store(nullptr, std::memory_order_release);
    self.research_user.store(user, std::memory_order_relaxed);
    self.research_draw.store(after, std::memory_order_release);
    self.research_before.store(before, std::memory_order_release);
    self.research_compute.store(compute, std::memory_order_release);
    refresh_shadow_need(self);
    return RSF_FRAME_TAP_OK;
}

extern "C" void* rsf_frame_tap_bound_target(void)
{
    return tap().target_texture;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_constant_override(
    rsf_frame_tap_constant_override_fn fn, void* user)
{
    Tap& self = tap();
    self.constant_override_user.store(user, std::memory_order_relaxed);
    self.constant_override.store(fn, std::memory_order_release);
    refresh_shadow_need(self);
    return RSF_FRAME_TAP_OK;
}

extern "C" void rsf_frame_tap_set_constant_override_format(uint32_t format)
{
    tap().constant_override_format.store(format, std::memory_order_release);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_divert(
    const rsf_frame_tap_divert_setup* setup)
{
    Tap& self = tap();
    if (!setup || !setup->layer_target || !setup->verdict) {
        // Disarmed before the layer is dropped, so a draw in flight cannot find a target that is
        // about to go away.
        self.divert_armed.store(0, std::memory_order_release);
        self.layer_target = nullptr;
        self.layer_texture = nullptr;
        self.verdict = nullptr;
        self.verdict_user = nullptr;
        refresh_shadow_need(self);
        return RSF_FRAME_TAP_OK;
    }
    if (setup->struct_size < sizeof(rsf_frame_tap_divert_setup)) {
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }

    auto* target = static_cast<ID3D11RenderTargetView*>(setup->layer_target);
    // The texture behind the view, resolved once here so the "already the layer" refusal is a
    // pointer compare at the draw rather than a resource query.
    ID3D11Resource* resource = nullptr;
    ID3D11Texture2D* texture = nullptr;
    target->GetResource(&resource);
    if (resource) {
        resource->QueryInterface(__uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&texture));
        resource->Release();
        if (texture) {
            texture->Release();  // borrowed: the layer owns it and outlives this
        }
    }

    self.layer_target = target;
    self.layer_texture = texture;
    self.layer_width = setup->layer_width;
    self.layer_height = setup->layer_height;
    self.verdict = setup->verdict;
    self.verdict_user = setup->verdict_user;
    self.divert_armed.store(1, std::memory_order_release);
    refresh_shadow_need(self);
    return RSF_FRAME_TAP_OK;
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_candidates(
    const rsf_frame_tap_candidates* candidates)
{
    Tap& self = tap();
    if (!candidates) {
        // Disarm. The counts go to zero first so a draw in flight stops looking before the entries
        // it was looking at are touched.
        self.candidates_armed.store(0, std::memory_order_relaxed);
        self.candidate_layout_count.store(0, std::memory_order_release);
        self.candidate_widget_target_count.store(0, std::memory_order_release);
        self.candidate_shader_count.store(0, std::memory_order_release);
        return RSF_FRAME_TAP_OK;
    }
    if (candidates->struct_size < sizeof(rsf_frame_tap_candidates)) {
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }
    if (candidates->layout_count > Tap::max_candidates ||
        candidates->widget_target_count > Tap::max_candidates ||
        candidates->shader_count > Tap::max_candidates) {
        // Refused rather than truncated. A silently shortened set is a rule that stops matching
        // partway down, which is indistinguishable from a rule that is wrong.
        return RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT;
    }

    struct Copy {
        void* const* source;
        uint32_t count;
        void* destination;
        std::atomic<uint32_t>* published;
    };
    const Copy copies[] = {
        {candidates->layouts, candidates->layouts ? candidates->layout_count : 0u,
         self.candidate_layouts, &self.candidate_layout_count},
        {candidates->widget_targets,
         candidates->widget_targets ? candidates->widget_target_count : 0u,
         self.candidate_widget_targets, &self.candidate_widget_target_count},
        {candidates->shaders, candidates->shaders ? candidates->shader_count : 0u,
         self.candidate_shaders, &self.candidate_shader_count},
    };

    uint32_t total = 0;
    for (const Copy& copy : copies) {
        // Shrink first, then write, then publish. A reader between the two sees fewer entries than
        // there are, never an entry that is being overwritten.
        copy.published->store(0, std::memory_order_release);
        if (copy.source && copy.count) {
            std::memcpy(copy.destination, copy.source, copy.count * sizeof(void*));
        }
        copy.published->store(copy.count, std::memory_order_release);
        total += copy.count;
    }
    self.candidates_armed.store(total != 0 ? 1u : 0u, std::memory_order_relaxed);
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
    status->depth_mismatches = self.depth_mismatches.load(std::memory_order_relaxed);
    status->depth_mismatch_target_width =
        self.depth_mismatch_target_width.load(std::memory_order_relaxed);
    status->depth_mismatch_target_height =
        self.depth_mismatch_target_height.load(std::memory_order_relaxed);
    status->depth_mismatch_depth_width =
        self.depth_mismatch_depth_width.load(std::memory_order_relaxed);
    status->depth_mismatch_depth_height =
        self.depth_mismatch_depth_height.load(std::memory_order_relaxed);
    status->depth_mismatch_depth_format =
        self.depth_mismatch_depth_format.load(std::memory_order_relaxed);
    status->candidate_draws =
        static_cast<uint32_t>(self.candidate_draws.load(std::memory_order_relaxed));
    status->draws_diverted =
        static_cast<uint32_t>(self.draws_diverted.load(std::memory_order_relaxed));
    status->blend_states_patched =
        static_cast<uint32_t>(self.blend_states_patched.load(std::memory_order_relaxed));
    status->divert_refused =
        static_cast<uint32_t>(self.divert_refused.load(std::memory_order_relaxed));
    status->divert_last_refusal = self.divert_last_refusal.load(std::memory_order_relaxed);
    status->vtable_refreshes = self.vtable_refreshes.load(std::memory_order_relaxed);
    status->draws_overridden = self.draws_overridden.load(std::memory_order_relaxed);
    status->copies_redirected = self.copies_redirected.load(std::memory_order_relaxed);
    status->copies_mismatched = self.copies_mismatched.load(std::memory_order_relaxed);
    status->updates_watched = self.updates_watched.load(std::memory_order_relaxed);
    status->gates_declined = self.gates_declined.load(std::memory_order_relaxed);
    return RSF_FRAME_TAP_OK;
}
