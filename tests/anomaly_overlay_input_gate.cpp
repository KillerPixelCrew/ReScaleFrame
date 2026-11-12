// SPDX-License-Identifier: GPL-3.0-only
// Contract for the Anomaly overlay input gate against a synthetic IDirectInputDevice8A. The stub
// honours the DirectInput buffer, acquisition and flush rules the engine's xr_input.cpp relies on
// (buffered GetDeviceData for both mouse and keyboard, Acquire on DIERR_NOTACQUIRED). No real
// device, no game and no Runtime are involved.
#include "overlay_input_gate.h"
#include <cstdio>
#include <cstring>
#include <deque>

using namespace rsf::anomaly::overlay;

namespace {
int failures = 0;
void check(bool condition, const char* what) {
    if (!condition) { std::printf("FAIL: %s\n", what); ++failures; }
}

struct Stub final : IDirectInputDevice8A {
    std::deque<DIDEVICEOBJECTDATA> events;
    DIMOUSESTATE mouse{};
    bool acquired = true;
    int acquires = 0, unacquires = 0, flushes = 0, data_reads = 0, state_reads = 0, coop_calls = 0;
    // Which method a raw vtable call reached; used to pin the slot numbers the hooks rely on.
    int last_method = 0;

    STDMETHOD(QueryInterface)(REFIID, LPVOID*) override { return E_NOTIMPL; }
    STDMETHOD_(ULONG, AddRef)() override { return 1; }
    STDMETHOD_(ULONG, Release)() override { return 1; }
    STDMETHOD(GetCapabilities)(LPDIDEVCAPS) override { return E_NOTIMPL; }
    STDMETHOD(EnumObjects)(LPDIENUMDEVICEOBJECTSCALLBACKA, LPVOID, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(GetProperty)(REFGUID, LPDIPROPHEADER) override { return E_NOTIMPL; }
    STDMETHOD(SetProperty)(REFGUID, LPCDIPROPHEADER) override { return E_NOTIMPL; }
    STDMETHOD(Acquire)() override { last_method = 7; ++acquires; acquired = true; return DI_OK; }
    STDMETHOD(Unacquire)() override { last_method = 8; ++unacquires; acquired = false; return DI_OK; }
    STDMETHOD(GetDeviceState)(DWORD size, LPVOID data) override {
        last_method = 9; ++state_reads;
        if (!acquired) return DIERR_NOTACQUIRED;
        std::memcpy(data, &mouse, size < sizeof(mouse) ? size : sizeof(mouse));
        mouse.lX = mouse.lY = mouse.lZ = 0;  // relative axes are consumed by a read
        return DI_OK;
    }
    STDMETHOD(GetDeviceData)(DWORD, LPDIDEVICEOBJECTDATA data, LPDWORD count, DWORD flags) override {
        last_method = 10; ++data_reads;
        if (!acquired) return DIERR_NOTACQUIRED;
        if (!data) {
            if (*count == INFINITE) { events.clear(); ++flushes; }
            return DI_OK;
        }
        DWORD copied = 0;
        while (copied < *count && copied < events.size()) { data[copied] = events[copied]; ++copied; }
        if (!(flags & DIGDD_PEEK)) events.erase(events.begin(), events.begin() + copied);
        *count = copied;
        return DI_OK;
    }
    STDMETHOD(SetDataFormat)(LPCDIDATAFORMAT) override { return E_NOTIMPL; }
    STDMETHOD(SetEventNotification)(HANDLE) override { return E_NOTIMPL; }
    STDMETHOD(SetCooperativeLevel)(HWND, DWORD) override { last_method = 13; ++coop_calls; return DI_OK; }
    STDMETHOD(GetObjectInfo)(LPDIDEVICEOBJECTINSTANCEA, DWORD, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(GetDeviceInfo)(LPDIDEVICEINSTANCEA) override { return E_NOTIMPL; }
    STDMETHOD(RunControlPanel)(HWND, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(Initialize)(HINSTANCE, DWORD, REFGUID) override { return E_NOTIMPL; }
    STDMETHOD(CreateEffect)(REFGUID, LPCDIEFFECT, LPDIRECTINPUTEFFECT*, LPUNKNOWN) override { return E_NOTIMPL; }
    STDMETHOD(EnumEffects)(LPDIENUMEFFECTSCALLBACKA, LPVOID, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(GetEffectInfo)(LPDIEFFECTINFOA, REFGUID) override { return E_NOTIMPL; }
    STDMETHOD(GetForceFeedbackState)(LPDWORD) override { return E_NOTIMPL; }
    STDMETHOD(SendForceFeedbackCommand)(DWORD) override { return E_NOTIMPL; }
    STDMETHOD(EnumCreatedEffectObjects)(LPDIENUMCREATEDEFFECTOBJECTSCALLBACK, LPVOID, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(Escape)(LPDIEFFESCAPE) override { return E_NOTIMPL; }
    STDMETHOD(Poll)() override { return E_NOTIMPL; }
    STDMETHOD(SendDeviceData)(DWORD, LPCDIDEVICEOBJECTDATA, LPDWORD, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(EnumEffectsInFile)(LPCSTR, LPDIENUMEFFECTSINFILECALLBACK, LPVOID, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(WriteEffectToFile)(LPCSTR, DWORD, LPDIFILEEFFECT, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(BuildActionMap)(LPDIACTIONFORMATA, LPCSTR, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(SetActionMap)(LPDIACTIONFORMATA, LPCSTR, DWORD) override { return E_NOTIMPL; }
    STDMETHOD(GetImageInfo)(LPDIDEVICEIMAGEINFOHEADERA) override { return E_NOTIMPL; }
};

IDirectInputDevice8A* iface(Stub& stub) { return &stub; }
Stub& stub_of(void* device) { return *static_cast<Stub*>(static_cast<IDirectInputDevice8A*>(device)); }

// The trampolines a production hook would pass: straight calls to the stub's own methods.
HRESULT STDMETHODCALLTYPE real_acquire(void* d) { return static_cast<IDirectInputDevice8A*>(d)->Acquire(); }
HRESULT STDMETHODCALLTYPE real_unacquire(void* d) { return static_cast<IDirectInputDevice8A*>(d)->Unacquire(); }
HRESULT STDMETHODCALLTYPE real_get_state(void* d, DWORD size, void* data) {
    return static_cast<IDirectInputDevice8A*>(d)->GetDeviceState(size, data);
}
HRESULT STDMETHODCALLTYPE real_get_data(void* d, DWORD size, DIDEVICEOBJECTDATA* data, DWORD* count, DWORD flags) {
    return static_cast<IDirectInputDevice8A*>(d)->GetDeviceData(size, data, count, flags);
}
const DeviceCalls calls{real_acquire, real_unacquire, real_get_state, real_get_data};

uint32_t current_capture = 0;
uint32_t capture(void*) noexcept { return current_capture; }

DIDEVICEOBJECTDATA event(DWORD ofs, DWORD data) {
    DIDEVICEOBJECTDATA value{};
    value.dwOfs = ofs; value.dwData = data; value.dwTimeStamp = 1;
    return value;
}
HRESULT read(InputGate& gate, void* device, DIDEVICEOBJECTDATA* out, DWORD& count, DWORD flags = 0) {
    return gate.get_data(device, calls, sizeof(DIDEVICEOBJECTDATA), out, &count, flags);
}

constexpr DWORD key_w = 0x11, key_a = 0x1e;

void slot_numbers_match_the_com_vtable() {
    Stub stub;
    auto** table = *reinterpret_cast<void***>(iface(stub));
    DWORD count = 0;
    DIMOUSESTATE state{};
    reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*)>(table[7])(iface(stub));
    check(stub.last_method == 7, "vtable slot 7 is Acquire");
    reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*)>(table[8])(iface(stub));
    check(stub.last_method == 8, "vtable slot 8 is Unacquire");
    stub.acquired = true;
    reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, DWORD, void*)>(table[9])(iface(stub), sizeof(state), &state);
    check(stub.last_method == 9, "vtable slot 9 is GetDeviceState");
    reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, DWORD, DIDEVICEOBJECTDATA*, DWORD*, DWORD)>(table[10])(
        iface(stub), sizeof(DIDEVICEOBJECTDATA), nullptr, &count, 0);
    check(stub.last_method == 10, "vtable slot 10 is GetDeviceData");
    reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, HWND, DWORD)>(table[13])(iface(stub), nullptr, 0);
    check(stub.last_method == 13, "vtable slot 13 is SetCooperativeLevel");
}

void closed_panel_passes_everything_through() {
    current_capture = 0;
    InputGate gate(capture, nullptr);
    Stub mouse, keyboard;
    gate.add(iface(mouse), DeviceKind::Mouse);
    gate.add(iface(keyboard), DeviceKind::Keyboard);
    mouse.events = {event(DIMOFS_X, 7), event(DIMOFS_BUTTON0, 0x80)};
    mouse.mouse.lX = 9; mouse.mouse.rgbButtons[0] = 0x80;
    keyboard.events = {event(key_w, 0x80)};
    DIDEVICEOBJECTDATA out[8]{};
    DWORD count = 8;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 2 && out[0].dwOfs == DIMOFS_X && out[0].dwData == 7 &&
        out[1].dwOfs == DIMOFS_BUTTON0 && out[1].dwData == 0x80, "mouse events arrive unchanged");
    count = 8;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 1 && out[0].dwOfs == key_w, "keyboard events arrive unchanged");
    DIMOUSESTATE state{};
    check(gate.get_state(iface(mouse), calls, sizeof(state), &state) == DI_OK && state.lX == 9 && state.rgbButtons[0] == 0x80,
        "mouse state arrives unchanged");
    check(mouse.flushes == 0 && keyboard.flushes == 0, "no flush while the panel is closed");
    check(gate.acquire(iface(mouse), calls) == DI_OK && mouse.acquires == 1, "Acquire reaches the device");
    check(gate.unacquire(iface(mouse), calls) == DI_OK && mouse.unacquires == 1, "Unacquire reaches the device");
    mouse.acquired = false;
    count = 8;
    check(read(gate, iface(mouse), out, count) == DIERR_NOTACQUIRED, "real device errors reach the engine");
}

void untracked_devices_are_never_touched() {
    current_capture = capture_mouse | capture_keyboard;
    InputGate gate(capture, nullptr);
    Stub other;
    other.events = {event(5, 0x80)};
    DIDEVICEOBJECTDATA out[2]{};
    DWORD count = 2;
    check(read(gate, iface(other), out, count) == DI_OK && count == 1 && other.flushes == 0, "untracked device passes through while captured");
    check(!gate.tracked(iface(other)), "not tracked");
}

void captured_mouse_sees_nothing_and_edges_flush() {
    current_capture = 0;
    InputGate gate(capture, nullptr);
    Stub mouse;
    gate.add(iface(mouse), DeviceKind::Mouse);
    DIDEVICEOBJECTDATA out[8]{};
    DWORD count = 8;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0, "idle read while closed");
    mouse.events = {event(DIMOFS_X, 5), event(DIMOFS_Y, 3), event(DIMOFS_BUTTON0, 0x80)};
    mouse.mouse.lX = 5; mouse.mouse.rgbButtons[0] = 0x80;
    current_capture = capture_mouse;
    count = 8;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0, "captured frame discards mouse deltas and buttons");
    check(mouse.flushes >= 1 && mouse.events.empty(), "rising edge flushed the device buffer");
    const int rising_flushes = mouse.flushes;
    DIMOUSESTATE state{};
    check(gate.get_state(iface(mouse), calls, sizeof(state), &state) == DI_OK && state.lX == 0 && state.rgbButtons[0] == 0,
        "captured GetDeviceState reports an idle mouse");
    check(mouse.mouse.lX == 0, "captured GetDeviceState still consumed the real delta");
    mouse.events = {event(DIMOFS_X, 11)};
    count = 8;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0 && mouse.events.empty(), "every captured frame drains the real buffer");
    check(mouse.flushes == rising_flushes + 1, "steady captured frames drain without counting as an edge");
    current_capture = 0;
    mouse.events = {event(DIMOFS_X, 99)};
    count = 8;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0 && mouse.flushes == rising_flushes + 2,
        "falling edge flushed what arrived while the panel was open");
    mouse.events = {event(DIMOFS_Y, 4)};
    count = 8;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 1 && out[0].dwOfs == DIMOFS_Y && out[0].dwData == 4,
        "input flows again after the panel closes");
}

void capture_is_per_device_kind() {
    InputGate gate(capture, nullptr);
    Stub mouse, keyboard;
    gate.add(iface(mouse), DeviceKind::Mouse);
    gate.add(iface(keyboard), DeviceKind::Keyboard);
    keyboard.events = {event(key_w, 0x80)};
    mouse.events = {event(DIMOFS_X, 1)};
    DIDEVICEOBJECTDATA out[4]{};
    current_capture = capture_mouse;
    DWORD count = 4;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 1, "keyboard unaffected by a mouse-only capture");
    count = 4;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0, "mouse withheld");
    current_capture = capture_keyboard;
    keyboard.events = {event(key_a, 0x80)};
    count = 4;
    // W was seen pressed above, so the edge hands the engine its release and nothing else.
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 1 && out[0].dwOfs == key_w && !(out[0].dwData & 0x80),
        "keyboard withheld by a keyboard capture (only the release of a held key arrives)");
    keyboard.events = {event(key_a, 0x80)};
    count = 4;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 0, "later keyboard events stay withheld");
    count = 4;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0, "mouse falling edge flushed what the panel held back");
    mouse.events = {event(DIMOFS_X, 2)};
    count = 4;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 1 && out[0].dwData == 2, "mouse flows during a keyboard-only capture");
}

void held_keys_and_buttons_are_released_when_capture_begins() {
    InputGate gate(capture, nullptr);
    Stub keyboard, mouse;
    gate.add(iface(keyboard), DeviceKind::Keyboard);
    gate.add(iface(mouse), DeviceKind::Mouse);
    current_capture = 0;
    keyboard.events = {event(key_w, 0x80), event(key_a, 0x80), event(key_a, 0)};
    mouse.events = {event(DIMOFS_BUTTON0, 0x80), event(DIMOFS_X, 3)};
    DIDEVICEOBJECTDATA out[8]{};
    DWORD count = 8;
    read(gate, iface(keyboard), out, count);
    count = 8;
    read(gate, iface(mouse), out, count);
    current_capture = capture_keyboard | capture_mouse;
    count = 8;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 1 && out[0].dwOfs == key_w && !(out[0].dwData & 0x80),
        "only the key still down gets a synthetic release");
    count = 8;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 0, "the release is sent once");
    count = 8;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 1 && out[0].dwOfs == DIMOFS_BUTTON0 && !(out[0].dwData & 0x80),
        "a held mouse button gets a synthetic release");
    // A key released while captured must not be reported again after the panel closes.
    keyboard.events = {event(key_w, 0x80)};
    current_capture = 0;
    count = 8;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 0, "presses made behind the panel are dropped");
}

void pending_releases_respect_capacity_and_peek() {
    InputGate gate(capture, nullptr);
    Stub keyboard;
    gate.add(iface(keyboard), DeviceKind::Keyboard);
    current_capture = 0;
    keyboard.events = {event(key_w, 0x80), event(key_a, 0x80)};
    DIDEVICEOBJECTDATA out[4]{};
    DWORD count = 4;
    read(gate, iface(keyboard), out, count);
    current_capture = capture_keyboard;
    count = 1;
    check(read(gate, iface(keyboard), out, count, DIGDD_PEEK) == DI_OK && count == 1, "peek reports a pending release");
    count = 1;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 1 && out[0].dwOfs == key_w, "first release delivered (lowest code)");
    count = 1;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 1 && out[0].dwOfs == key_a, "second release delivered on the next call");
    count = 1;
    check(read(gate, iface(keyboard), out, count) == DI_OK && count == 0, "nothing left to release");
}

void exclusive_mouse_is_handed_to_the_window() {
    InputGate gate(capture, nullptr);
    Stub mouse;
    gate.add(iface(mouse), DeviceKind::Mouse);
    gate.cooperative_level(iface(mouse), DISCL_EXCLUSIVE | DISCL_FOREGROUND);
    current_capture = 0;
    check(gate.acquire(iface(mouse), calls) == DI_OK && mouse.acquired && mouse.acquires == 1, "engine acquires while closed");
    DIDEVICEOBJECTDATA out[4]{};
    DWORD count = 4;
    current_capture = capture_mouse;
    mouse.events = {event(DIMOFS_X, 8)};
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0, "captured read is quiet");
    check(!mouse.acquired && mouse.unacquires == 1, "the exclusive mouse was unacquired so the window gets WM_MOUSE*");
    count = 4;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 0, "a gate-unacquired mouse does not report NOTACQUIRED to the engine");
    DIMOUSESTATE state{};
    check(gate.get_state(iface(mouse), calls, sizeof(state), &state) == DI_OK, "GetDeviceState likewise");
    check(gate.acquire(iface(mouse), calls) == DI_OK && mouse.acquires == 1 && !mouse.acquired, "the engine's own Acquire is answered without acquiring");
    current_capture = 0;
    mouse.acquired = false;
    count = 4;
    mouse.events.clear();
    check(read(gate, iface(mouse), out, count) == DI_OK && mouse.acquired && mouse.acquires == 2, "falling edge reacquires the mouse");
    mouse.events = {event(DIMOFS_Y, 2)};
    count = 4;
    check(read(gate, iface(mouse), out, count) == DI_OK && count == 1 && out[0].dwOfs == DIMOFS_Y, "mouse look resumes after close");
}

void exclusive_mouse_the_engine_released_is_not_reacquired() {
    InputGate gate(capture, nullptr);
    Stub mouse;
    gate.add(iface(mouse), DeviceKind::Mouse);
    gate.cooperative_level(iface(mouse), DISCL_EXCLUSIVE | DISCL_FOREGROUND);
    current_capture = 0;
    gate.acquire(iface(mouse), calls);
    gate.unacquire(iface(mouse), calls);  // the engine let go (for example a menu)
    const int unacquires = mouse.unacquires;
    current_capture = capture_mouse;
    DIDEVICEOBJECTDATA out[2]{};
    DWORD count = 2;
    mouse.acquired = false;
    read(gate, iface(mouse), out, count);
    current_capture = 0;
    count = 2;
    read(gate, iface(mouse), out, count);
    check(mouse.unacquires == unacquires && mouse.acquires == 1, "a mouse the engine released is neither unacquired nor reacquired by the gate");
}

void non_exclusive_mouse_stays_acquired() {
    InputGate gate(capture, nullptr);
    Stub mouse;
    gate.add(iface(mouse), DeviceKind::Mouse);
    gate.cooperative_level(iface(mouse), DISCL_NONEXCLUSIVE | DISCL_FOREGROUND);
    current_capture = 0;
    gate.acquire(iface(mouse), calls);
    current_capture = capture_mouse;
    DIDEVICEOBJECTDATA out[2]{};
    DWORD count = 2;
    read(gate, iface(mouse), out, count);
    check(mouse.acquired && mouse.unacquires == 0, "a non-exclusive mouse is left alone");
    current_capture = 0;
}

void lost_keyboard_reaches_the_engine_even_while_captured() {
    InputGate gate(capture, nullptr);
    Stub keyboard;
    gate.add(iface(keyboard), DeviceKind::Keyboard);
    current_capture = capture_keyboard;
    keyboard.acquired = false;
    DIDEVICEOBJECTDATA out[2]{};
    DWORD count = 2;
    check(read(gate, iface(keyboard), out, count) == DIERR_NOTACQUIRED, "the engine still learns a keyboard was lost and reacquires");
    DIMOUSESTATE unused{};
    check(gate.get_state(iface(keyboard), calls, 256, &unused) == DIERR_NOTACQUIRED, "GetDeviceState likewise");
    current_capture = 0;
}

void engine_flush_requests_pass_through() {
    InputGate gate(capture, nullptr);
    Stub keyboard;
    gate.add(iface(keyboard), DeviceKind::Keyboard);
    current_capture = capture_keyboard;
    DWORD count = INFINITE;
    keyboard.events = {event(key_w, 0x80)};
    check(gate.get_data(iface(keyboard), calls, sizeof(DIDEVICEOBJECTDATA), nullptr, &count, 0) == DI_OK, "engine NULL flush accepted");
    check(keyboard.events.empty(), "and executed by the device");
    current_capture = 0;
}
}

int main() {
    slot_numbers_match_the_com_vtable();
    closed_panel_passes_everything_through();
    untracked_devices_are_never_touched();
    captured_mouse_sees_nothing_and_edges_flush();
    capture_is_per_device_kind();
    held_keys_and_buttons_are_released_when_capture_begins();
    pending_releases_respect_capacity_and_peek();
    exclusive_mouse_is_handed_to_the_window();
    exclusive_mouse_the_engine_released_is_not_reacquired();
    non_exclusive_mouse_stays_acquired();
    lost_keyboard_reaches_the_engine_even_while_captured();
    engine_flush_requests_pass_through();
    if (failures) { std::printf("%d failure(s)\n", failures); return 1; }
    std::puts("Anomaly overlay input gate: PASS");
    return 0;
}
