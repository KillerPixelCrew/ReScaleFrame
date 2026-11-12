// SPDX-License-Identifier: GPL-3.0-only
#include "overlay_input_gate.h"
#include <cstring>

namespace rsf::anomaly::overlay {
namespace {
constexpr DWORD dx3_object_size = 16;  // DIDEVICEOBJECTDATA_DX3: no dwSequence/uAppData
bool well_formed(DWORD stride) noexcept { return stride == sizeof(DIDEVICEOBJECTDATA) || stride == dx3_object_size; }
bool input_lost(HRESULT result) noexcept { return result == DIERR_INPUTLOST || result == DIERR_NOTACQUIRED; }
DIDEVICEOBJECTDATA* at(DIDEVICEOBJECTDATA* base, DWORD index, DWORD stride) noexcept {
    return reinterpret_cast<DIDEVICEOBJECTDATA*>(reinterpret_cast<BYTE*>(base) + size_t(index) * stride);
}
}

void InputGate::add(void* device, DeviceKind kind) {
    if (!device) return;
    std::lock_guard lock(guard_);
    Entry entry;
    entry.kind = kind;
    devices_[device] = entry;
}
void InputGate::remove(void* device) {
    std::lock_guard lock(guard_);
    devices_.erase(device);
}
bool InputGate::tracked(void* device) {
    std::lock_guard lock(guard_);
    return devices_.find(device) != devices_.end();
}
void InputGate::cooperative_level(void* device, DWORD flags) {
    std::lock_guard lock(guard_);
    if (const auto it = devices_.find(device); it != devices_.end()) it->second.exclusive = (flags & DISCL_EXCLUSIVE) != 0;
}

// Runs on the thread that polls the device, before the call is answered. Returns whether the device
// kind is captured now. The edge work is the same in both directions: drop whatever is buffered.
bool InputGate::update(Entry& entry, void* device, const DeviceCalls& calls) {
    const uint32_t bits = capture_ ? capture_(user_) : 0u;
    const bool now = (bits & (entry.kind == DeviceKind::Mouse ? capture_mouse : capture_keyboard)) != 0;
    if (now == entry.captured) return now;
    entry.captured = now;
    DWORD flushed = INFINITE;
    (void)calls.get_data(device, sizeof(DIDEVICEOBJECTDATA), nullptr, &flushed, 0);  // NULL + INFINITE clears the buffer
    if (now) {
        entry.release_pending |= entry.down;
        entry.down.reset();
        if (entry.kind == DeviceKind::Mouse && entry.exclusive && entry.game_acquired && !entry.gate_unacquired) {
            (void)calls.unacquire(device);
            entry.gate_unacquired = true;
        }
    } else if (entry.gate_unacquired) {
        entry.gate_unacquired = false;
        // A refusal (window not foreground) leaves the engine to see NOTACQUIRED and acquire itself.
        if (entry.game_acquired) (void)calls.acquire(device);
    }
    return now;
}

uint32_t InputGate::emit_pending(Entry& entry, DIDEVICEOBJECTDATA* out, DWORD capacity, DWORD stride, bool consume) {
    if (entry.release_pending.none() || !out || !capacity || !well_formed(stride)) return 0;
    uint32_t written = 0;
    const DWORD now = GetTickCount();
    for (size_t ofs = 0; ofs < entry.release_pending.size() && written < capacity; ++ofs) {
        if (!entry.release_pending.test(ofs)) continue;
        auto* slot = at(out, written, stride);
        std::memset(slot, 0, stride);
        slot->dwOfs = static_cast<DWORD>(ofs);
        slot->dwData = 0;  // bit 7 clear: released
        slot->dwTimeStamp = now;
        if (consume) entry.release_pending.reset(ofs);
        ++written;
    }
    return written;
}

void InputGate::track(Entry& entry, const DIDEVICEOBJECTDATA* events, DWORD count, DWORD stride) {
    if (!well_formed(stride)) return;
    for (DWORD i = 0; i < count; ++i) {
        const auto* event = reinterpret_cast<const DIDEVICEOBJECTDATA*>(reinterpret_cast<const BYTE*>(events) + size_t(i) * stride);
        const DWORD ofs = event->dwOfs;
        const bool relevant = entry.kind == DeviceKind::Keyboard ? ofs < 256 : (ofs >= DIMOFS_BUTTON0 && ofs <= DIMOFS_BUTTON7);
        if (relevant) entry.down.set(ofs, (event->dwData & 0x80) != 0);
    }
}

HRESULT InputGate::acquire(void* device, const DeviceCalls& calls) {
    std::unique_lock lock(guard_);
    const auto it = devices_.find(device);
    if (it == devices_.end()) { lock.unlock(); return calls.acquire(device); }
    Entry& entry = it->second;
    entry.game_acquired = true;
    const bool captured = update(entry, device, calls);
    // The engine believes it holds the mouse; the window keeps it until the panel closes.
    if (entry.gate_unacquired) return DI_OK;
    if (captured && entry.kind == DeviceKind::Mouse && entry.exclusive) { entry.gate_unacquired = true; return DI_OK; }
    return calls.acquire(device);
}

HRESULT InputGate::unacquire(void* device, const DeviceCalls& calls) {
    std::unique_lock lock(guard_);
    if (const auto it = devices_.find(device); it != devices_.end()) {
        it->second.game_acquired = false;
        it->second.gate_unacquired = false;
    }
    lock.unlock();
    return calls.unacquire(device);
}

HRESULT InputGate::get_state(void* device, const DeviceCalls& calls, DWORD size, void* data) {
    std::unique_lock lock(guard_);
    const auto it = devices_.find(device);
    if (it == devices_.end()) { lock.unlock(); return calls.get_state(device, size, data); }
    Entry& entry = it->second;
    if (!update(entry, device, calls) || !data || !size) return calls.get_state(device, size, data);
    // Read through so relative axes do not pile up behind the panel, then hide the result.
    const HRESULT result = entry.gate_unacquired ? DIERR_NOTACQUIRED : calls.get_state(device, size, data);
    if (!entry.gate_unacquired && FAILED(result)) return result;
    std::memset(data, 0, size);
    return DI_OK;
}

HRESULT InputGate::get_data(void* device, const DeviceCalls& calls, DWORD object_size,
    DIDEVICEOBJECTDATA* data, DWORD* count, DWORD flags) {
    std::unique_lock lock(guard_);
    const auto it = devices_.find(device);
    if (it == devices_.end()) { lock.unlock(); return calls.get_data(device, object_size, data, count, flags); }
    Entry& entry = it->second;
    const bool captured = update(entry, device, calls);
    // NULL data is a flush or query by the engine, not a read; the real device answers it.
    if (!data || !count) return calls.get_data(device, object_size, data, count, flags);
    const bool peek = (flags & DIGDD_PEEK) != 0;
    const DWORD capacity = *count;
    if (!well_formed(object_size)) {
        if (!captured) return calls.get_data(device, object_size, data, count, flags);
        DWORD flushed = INFINITE;
        (void)calls.get_data(device, object_size, nullptr, &flushed, 0);
        *count = 0;
        return DI_OK;
    }
    const uint32_t pending = emit_pending(entry, data, capacity, object_size, !peek);
    if (captured) {
        HRESULT drained = DI_OK;
        if (!peek && !entry.gate_unacquired) {
            DWORD discarded = INFINITE;
            drained = calls.get_data(device, object_size, nullptr, &discarded, 0);
        }
        *count = pending;
        if (pending) return DI_OK;
        return (!entry.gate_unacquired && input_lost(drained)) ? drained : DI_OK;
    }
    if (pending >= capacity) { *count = pending; return DI_OK; }
    DWORD received = capacity - pending;
    const HRESULT result = calls.get_data(device, object_size, at(data, pending, object_size), &received, flags);
    if (FAILED(result)) {
        if (!pending) return result;
        *count = pending;
        return DI_OK;
    }
    if (!peek) track(entry, at(data, pending, object_size), received, object_size);
    *count = pending + received;
    return result;
}
}
