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
constexpr size_t slot_ps_set_constant_buffers = 16;

using ps_set_shader_resources_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                            ID3D11ShaderResourceView* const*);
using ps_set_constant_buffers_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                            ID3D11Buffer* const*);

// The set that identifies this pass is five views. A pixel shader may bind far more, so the
// examined window is bounded to keep the stack frame and the loop fixed on a path that runs
// hundreds of times a frame. Chosen rather than measured: no capture shows the set bound above
// slot 15, but nothing guarantees a later one will not.
constexpr UINT max_examined_views = 16;

struct Tap {
    std::mutex guard;
    bool installed = false;

    void** vtable = nullptr;
    ps_set_shader_resources_fn original_set_views = nullptr;
    ps_set_constant_buffers_fn original_set_constants = nullptr;

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
    // Edge trigger. The signature stays complete across the draws that use it, and firing per call
    // would run a backend several times on one frame.
    bool signature_complete = false;

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
    // Forwarded first, always. The game's binding has to happen whether or not this recognises it,
    // and it has to be in place before a callback that may bind something else and restore it.
    const ps_set_shader_resources_fn forward = self.original_set_views;
    if (!forward) {
        // Only reachable if a call arrived between the vtable write and the store of the original,
        // which the write order in patch_slot rules out on the architectures this ships on.
        // Dropping a binding is wrong and will show; calling through a null pointer ends the
        // process with nothing to read.
        return;
    }
    forward(context, start_slot, count, views);

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

    for (UINT index = 0; index < max_examined_views; ++index) {
        Tap::Slot& entry = self.slots[index];
        if (!entry.texture) {
            continue;
        }
        entry.role = role_of(entry.description, shape);
        // Slot 0 is taken to be scene colour, which is what Unreal's post process input convention
        // gives, and several full resolution targets in this frame share its descriptor so nothing
        // in the binding distinguishes it. This cannot be checked without the running game.
        if (index == 0) {
            scene_color = entry.texture;
            continue;
        }
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

    // Velocity, a 1x1 target and depth bound at the same time is the signature. Format rules for
    // each live in resource_roles.cpp so that this module and the classifier cannot drift apart.
    //
    // Edge triggered: the set stays bound across the draws that use it, and firing on every call
    // while it does would run a backend several times over one frame.
    const bool qualifies = motion && depth && exposure;
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

    // Nothing is released here. The shadow now owns one reference per occupied slot, taken when the
    // slot changed and dropped when it changes again or when the tap is uninstalled. Releasing per
    // call was right while the set had to arrive in one call, and would be a use after free now
    // that the entries have to survive until the slot is rebound.
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

    if (!patch_slot(self.vtable, slot_ps_set_shader_resources,
                    reinterpret_cast<void*>(&hooked_ps_set_shader_resources),
                    reinterpret_cast<void**>(&self.original_set_views))) {
        return RSF_FRAME_TAP_ERROR_PATCH_FAILED;
    }
    if (!patch_slot(self.vtable, slot_ps_set_constant_buffers,
                    reinterpret_cast<void*>(&hooked_ps_set_constant_buffers),
                    reinterpret_cast<void**>(&self.original_set_constants))) {
        patch_slot(self.vtable, slot_ps_set_shader_resources,
                   reinterpret_cast<void*>(self.original_set_views), nullptr);
        self.original_set_views = nullptr;
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
        patch_slot(self.vtable, slot_ps_set_shader_resources,
                   reinterpret_cast<void*>(self.original_set_views), nullptr);
        patch_slot(self.vtable, slot_ps_set_constant_buffers,
                   reinterpret_cast<void*>(self.original_set_constants), nullptr);
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
    self.signature_complete = false;
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
    return RSF_FRAME_TAP_OK;
}
