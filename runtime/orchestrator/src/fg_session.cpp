// SPDX-License-Identifier: GPL-3.0-only
// Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
// work (see docs/representation-plan.md). Do not delete as dead code.
#include <rescaleframe/fg_session.h>
#include <rescaleframe/frame_sequencer.h>
#include <windows.h>
#include <d3d12.h>
#include <mutex>
#include <new>
struct rsf_fg_session {
    std::mutex guard;
    rsf_fg_host host{};
    const rsf_generation_provider* provider = nullptr;
    void* context = nullptr;
    rsf_frame_sequencer* ledger = nullptr;
    uint64_t prepared = 0;
    uint32_t in_flight = 0;
    bool transition = false;
    rsf_fg_session_status state{};
};
namespace {
bool valid_provider(const rsf_generation_provider* p)
{
    return p && p->struct_size >= sizeof(*p) && p->create && p->configure && p->begin_frame &&
        p->marker && p->prepare && p->after_present && p->status && p->retirement && p->destroy && p->abort_frame;
}
rsf_backend_result retire(rsf_fg_session& self)
{
    if (!self.provider) return RSF_BACKEND_OK;
    rsf_fg_retirement retirement{}; retirement.struct_size = sizeof(retirement);
    const auto result = self.provider->retirement(self.context, &retirement);
    if (result != 0) return result;
    if (!retirement.fence) return retirement.value ? RSF_BACKEND_ERROR_INVALID_ARGUMENT : RSF_BACKEND_OK;
    auto* fence = static_cast<ID3D12Fence*>(retirement.fence);
    if (fence->GetCompletedValue() == UINT64_MAX) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    // Keep the context and leases alive when the SDK has not yet finished reading them.
    return fence->GetCompletedValue() >= retirement.value ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result detach(rsf_fg_session& self)
{
    auto result = retire(self); if (result != 0) return result;
    result = self.host.release(self.host.user); if (result != 0) return result;
    if (self.provider) self.provider->destroy(self.context);
    self.context = nullptr; self.provider = nullptr; self.prepared = 0;
    self.state.has_provider = 0; self.state.provider = {};
    ++self.state.chain_generation;
    return RSF_BACKEND_OK;
}
}
extern "C" rsf_backend_result rsf_fg_session_create(const rsf_fg_host* host, rsf_fg_session** out)
{
    if (!host || !out || host->struct_size < sizeof(*host)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (host->abi_version != RSF_FG_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!host->quiesce || !host->resume || !host->release || !host->adopt || !host->restore_plain)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* self = new (std::nothrow) rsf_fg_session;
    if (!self) return RSF_BACKEND_ERROR_INIT_FAILED;
    auto result = rsf_frame_sequencer_create(8, &self->ledger);
    if (result != 0) { delete self; return result; }
    self->host = *host; self->state.struct_size = sizeof(self->state);
    *out = self; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_session_select(rsf_fg_session* self,
    const rsf_generation_provider* provider, const rsf_generation_setup* setup, const rsf_fg_options* options)
{
    if (!self) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (provider && (!valid_provider(provider) || !setup || !options || setup->struct_size < sizeof(*setup) ||
        options->struct_size < sizeof(*options))) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (provider && (setup->abi_version != RSF_FG_ABI_VERSION || options->abi_version != RSF_FG_ABI_VERSION))
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    std::unique_lock<std::mutex> lock(self->guard);
    if (self->in_flight || self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    self->transition = true;
    lock.unlock();
    auto result = self->host.quiesce(self->host.user);
    lock.lock();
    if (result != 0) { self->transition = false; self->state.last_switch = result; return result; }
    if (provider) {
        // Capability probe has no HWND and therefore cannot create a second physical chain.
        auto probe_setup = *setup; probe_setup.chain.hwnd = nullptr;
        void* probe = nullptr; void* probe_chain = nullptr;
        result = provider->create(&probe_setup, &probe, &probe_chain);
        if (result == 0) {
            rsf_fg_status capability{}; capability.struct_size = sizeof(capability);
            result = probe ? provider->status(probe, &capability) : RSF_BACKEND_ERROR_NOT_READY;
            if (result == 0 && (!capability.supported || probe_chain)) result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
            if (result == 0 && (options->mode > RSF_FG_DYNAMIC || options->reflex_mode > RSF_REFLEX_BOOST ||
                (options->mode == RSF_FG_FIXED && !options->generated_frames))) result = RSF_BACKEND_ERROR_INVALID_ARGUMENT;
            if (result == 0 && ((options->mode == RSF_FG_DYNAMIC && !capability.dynamic_supported) ||
                (options->mode == RSF_FG_AUTO && !capability.auto_supported) ||
                (options->mode == RSF_FG_FIXED && !capability.max_generated_frames))) result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
            if (result == 0 && options->reflex_mode != RSF_REFLEX_OFF && capability.pacing_owner != RSF_PACING_REFLEX)
                result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
            if (result == 0 && options->frame_limit_us && capability.pacing_owner == RSF_PACING_NONE)
                result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
        }
        if (probe) provider->destroy(probe);
        if (result != 0) {
            self->state.last_switch = result; self->host.resume(self->host.user);
            self->transition = false; return result;
        }
    }
    result = detach(*self);
    if (result != 0) {
        self->host.resume(self->host.user); self->state.last_switch = result;
        self->transition = false; return result;
    }
    self->state.plain_available = 0;
    if (provider) {
        void* context = nullptr; void* chain = nullptr;
        result = provider->create(setup, &context, &chain);
        if (result == 0 && (!context || !chain)) result = RSF_BACKEND_ERROR_NOT_READY;
        if (result == 0) result = provider->configure(context, options);
        if (result == 0) result = self->host.adopt(self->host.user, chain);
        if (result == 0) {
            self->provider = provider; self->context = context; self->state.has_provider = 1;
        } else if (context) {
            // adopt must be transactional: a failure cannot leave a borrowed chain retained.
            provider->destroy(context);
        }
    }
    if (!self->provider) {
        const auto fallback = self->host.restore_plain(self->host.user);
        self->state.plain_available = fallback == 0;
        if (fallback != 0) result = fallback;
    }
    self->state.last_switch = result;
    self->host.resume(self->host.user);
    self->transition = false;
    return result;
}
extern "C" rsf_backend_result rsf_fg_session_begin(rsf_fg_session* self, uint64_t id)
{
    if (!self) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(self->guard);
    if (self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    auto result = rsf_frame_sequencer_begin(self->ledger, id); if (result != 0) return result;
    if (self->provider) result = self->provider->begin_frame(self->context, id);
    if (result != 0) (void)rsf_frame_sequencer_finish(self->ledger, id);
    else ++self->in_flight;
    return result;
}
extern "C" rsf_backend_result rsf_fg_session_marker(rsf_fg_session* self, uint64_t id,
    rsf_latency_marker marker, uint64_t timestamp, uint32_t controller)
{
    if (!self || controller > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(self->guard);
    if (self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    auto result = rsf_frame_sequencer_marker(self->ledger, id, marker, timestamp);
    if (result == 0 && self->provider) result = self->provider->marker(self->context, marker, id, controller);
    return result;
}
extern "C" rsf_backend_result rsf_fg_session_prepare(rsf_fg_session* self, void* command, const rsf_fg_frame* frame)
{
    if (!self || !frame || frame->struct_size < sizeof(*frame) || !frame->record ||
        frame->record->struct_size < sizeof(*frame->record)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(self->guard);
    if (self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    if (frame->record->abi_version != RSF_GAME_FRAME_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    rsf_sequence_state sequence{}; sequence.struct_size = sizeof(sequence);
    auto result = rsf_frame_sequencer_get(self->ledger, frame->record->frame_id, &sequence);
    if (result != 0 || sequence.failed || (sequence.marker_mask != 15 && sequence.marker_mask != 31) || self->prepared)
        return RSF_BACKEND_ERROR_NOT_READY;
    auto prepared_frame = *frame;
    const auto& record = *frame->record;
    if (!rsf_frame_allows_fg(&record) || record.flags & (RSF_FRAME_FLAG_RESET | RSF_FRAME_FLAG_AMBIGUOUS_ID))
        prepared_frame.interpolate = 0;
    if (prepared_frame.interpolate && record.input_qpc != sequence.timestamps[0])
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (self->provider) result = self->provider->prepare(self->context, command, &prepared_frame);
    if (result == 0) self->prepared = frame->record->frame_id;
    self->state.last_frame = result; return result;
}
extern "C" rsf_backend_result rsf_fg_session_after_present(rsf_fg_session* self, uint64_t id)
{
    if (!self || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(self->guard);
    if (self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    if (self->prepared != id) return RSF_BACKEND_ERROR_NOT_READY;
    auto result = rsf_frame_sequencer_finish(self->ledger, id);
    self->prepared = 0;
    --self->in_flight;
    if (self->provider) {
        const auto vendor = self->provider->after_present(self->context);
        if (result == 0) result = vendor;
    }
    self->state.last_frame = result; return result;
}
extern "C" rsf_backend_result rsf_fg_session_abort(rsf_fg_session* self, uint64_t id)
{
    if (!self || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(self->guard);
    if (self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    rsf_sequence_state state{}; state.struct_size = sizeof(state);
    auto result = rsf_frame_sequencer_get(self->ledger, id, &state); if (result != 0) return result;
    if (self->provider) {
        result = self->provider->abort_frame(self->context, id); if (result != 0) return result;
    }
    (void)rsf_frame_sequencer_finish(self->ledger, id);
    if (self->prepared == id) self->prepared = 0;
    --self->in_flight; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_session_retirement(rsf_fg_session* self, rsf_fg_retirement* out)
{
    if (!self || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(self->guard);
    if (self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    out->fence = nullptr; out->value = 0;
    return self->provider ? self->provider->retirement(self->context, out) : RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_session_get_status(rsf_fg_session* self, rsf_fg_session_status* out)
{
    if (!self || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(self->guard);
    if (self->transition) return RSF_BACKEND_ERROR_NOT_READY;
    if (self->provider) {
        self->state.provider.struct_size = sizeof(self->state.provider);
        const auto result = self->provider->status(self->context, &self->state.provider);
        if (result != 0) return result;
    }
    *out = self->state; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_session_destroy(rsf_fg_session* self)
{
    if (!self) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto result = rsf_fg_session_select(self, nullptr, nullptr, nullptr);
    if (result != 0) return result;
    rsf_frame_sequencer_destroy(self->ledger); delete self; return RSF_BACKEND_OK;
}
